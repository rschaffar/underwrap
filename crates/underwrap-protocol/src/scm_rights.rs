//! SCM_RIGHTS helpers for passing file descriptors over Unix sockets.

use anyhow::{Result, bail};
use std::mem::{self, MaybeUninit};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream as StdUnixStream;

fn cmsg_space(payload_bytes: usize) -> usize {
    // SAFETY: these callers pass only the bounded sizes of RawFd and ucred arrays,
    // so the libc CMSG_SPACE arithmetic cannot overflow its u32 input or usize output.
    unsafe { libc::CMSG_SPACE(payload_bytes as u32) as usize }
}

fn cmsg_len(payload_bytes: usize) -> usize {
    // SAFETY: the same bounded-size invariant as cmsg_space applies.
    unsafe { libc::CMSG_LEN(payload_bytes as u32) as usize }
}

fn aligned_control_storage(control_bytes: usize) -> Vec<MaybeUninit<libc::cmsghdr>> {
    let headers = control_bytes.div_ceil(mem::size_of::<libc::cmsghdr>());
    vec![MaybeUninit::zeroed(); headers]
}

fn receive_control_bytes() -> usize {
    // Two descriptors let recv_fd distinguish an exact extra fd from success.
    // One ucred-sized message additionally lets it observe and reject the Linux
    // SCM_CREDENTIALS + SCM_RIGHTS mixed-control case without truncation.
    cmsg_space(mem::size_of::<[RawFd; 2]>()) + cmsg_space(mem::size_of::<libc::ucred>())
}

/// Send a file descriptor over a Unix stream socket using SCM_RIGHTS.
///
/// Also sends a single data byte (required — ancillary data is only
/// delivered alongside actual data on stream sockets).
pub fn send_fd(socket: BorrowedFd<'_>, fd: BorrowedFd<'_>) -> Result<()> {
    use libc::{CMSG_DATA, CMSG_FIRSTHDR, SCM_RIGHTS, SOL_SOCKET, c_void, iovec, msghdr, sendmsg};

    let data_byte: [u8; 1] = [0x42]; // Arbitrary marker byte.
    let mut iov = iovec {
        iov_base: data_byte.as_ptr() as *mut c_void,
        iov_len: 1,
    };

    let control_bytes = cmsg_space(mem::size_of::<RawFd>());
    let mut control = aligned_control_storage(control_bytes);

    // SAFETY: all-zero is a valid empty msghdr, and every pointer-bearing field
    // used by sendmsg is initialized below to live storage for the call.
    let mut msg: msghdr = unsafe { mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = control_bytes;

    // SAFETY: control is cmsghdr-aligned and at least CMSG_SPACE(sizeof(RawFd))
    // bytes long. CMSG_FIRSTHDR and CMSG_DATA therefore point within that live
    // storage, and sendmsg borrows both descriptors and all message buffers only
    // for this call.
    unsafe {
        let cmsg = CMSG_FIRSTHDR(&msg);
        (*cmsg).cmsg_level = SOL_SOCKET;
        (*cmsg).cmsg_type = SCM_RIGHTS;
        (*cmsg).cmsg_len = cmsg_len(mem::size_of::<RawFd>());
        let raw_fd = fd.as_raw_fd();
        std::ptr::copy_nonoverlapping(&raw_fd, CMSG_DATA(cmsg).cast::<RawFd>(), 1);

        let ret = sendmsg(socket.as_raw_fd(), &msg, 0);
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            bail!("sendmsg(SCM_RIGHTS) failed: {err}");
        }
    }

    Ok(())
}

/// Receive a file descriptor from a Unix stream socket using SCM_RIGHTS.
///
/// Returns the received descriptor; the required marker byte is consumed internally.
pub fn recv_fd(socket: BorrowedFd<'_>) -> Result<OwnedFd> {
    use libc::{
        CMSG_DATA, CMSG_FIRSTHDR, CMSG_NXTHDR, SCM_RIGHTS, SOL_SOCKET, c_void, iovec, msghdr,
        recvmsg,
    };

    let mut data_byte: [u8; 1] = [0];
    let mut iov = iovec {
        iov_base: data_byte.as_mut_ptr().cast::<c_void>(),
        iov_len: 1,
    };

    let control_bytes = receive_control_bytes();
    let mut control = aligned_control_storage(control_bytes);

    // SAFETY: all-zero is a valid empty msghdr, and every pointer-bearing field
    // used by recvmsg is initialized below to live writable storage for the call.
    let mut msg: msghdr = unsafe { mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = control_bytes;

    // SAFETY: socket remains borrowed and open, iov names one writable byte, and
    // control is cmsghdr-aligned writable storage of msg_controllen bytes.
    let ret = unsafe { recvmsg(socket.as_raw_fd(), &mut msg, libc::MSG_CMSG_CLOEXEC) };
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        bail!("recvmsg(SCM_RIGHTS) failed: {err}");
    }
    if ret == 0 {
        bail!("recvmsg: connection closed");
    }

    let mut received_fds = Vec::<OwnedFd>::new();
    let mut rights_messages = 0usize;
    let mut malformed_rights = false;
    let mut unexpected_control = false;

    // Adopt every descriptor the kernel exposed in SCM_RIGHTS control data
    // before checking truncation, cardinality, or unexpected message types.
    // Any later error therefore drops all installed descriptors in received_fds.
    // SAFETY: recvmsg writes only within the supplied cmsghdr-aligned control
    // allocation. Its reported length is clamped to that allocation, and before
    // dereferencing a header or payload the code below proves that its complete
    // bytes lie in the resulting initialized range.
    // CMSG_NXTHDR is called only for a structurally bounded current header, and
    // each bounded nonnegative RawFd is transferred exactly once into OwnedFd.
    unsafe {
        let control_start = msg.msg_control as usize;
        let reported_control_len = msg.msg_controllen;
        let control_len = reported_control_len.min(control_bytes);
        malformed_rights |= reported_control_len > control_bytes;
        msg.msg_controllen = control_len;
        let control_end = control_start
            .checked_add(control_len)
            .expect("control allocation address range cannot overflow");

        let mut cmsg = CMSG_FIRSTHDR(&msg);
        while !cmsg.is_null() {
            let cmsg_address = cmsg as usize;
            let Some(cmsg_offset) = cmsg_address.checked_sub(control_start) else {
                malformed_rights = true;
                break;
            };
            if cmsg_offset > control_len
                || mem::size_of::<libc::cmsghdr>() > control_len - cmsg_offset
            {
                malformed_rights = true;
                break;
            }

            let header = &*cmsg;
            let data_address = CMSG_DATA(cmsg) as usize;
            let Some(data_offset) = data_address.checked_sub(control_start) else {
                malformed_rights = true;
                break;
            };
            if data_address > control_end || data_offset < cmsg_offset {
                malformed_rights = true;
                break;
            }

            let available_cmsg_bytes = control_len - cmsg_offset;
            let available_payload_bytes = control_len - data_offset;
            let payload_offset = data_offset - cmsg_offset;
            let declared_cmsg_bounded = header.cmsg_len <= available_cmsg_bytes;
            malformed_rights |= header.cmsg_len < payload_offset || !declared_cmsg_bounded;

            if header.cmsg_level == SOL_SOCKET && header.cmsg_type == SCM_RIGHTS {
                rights_messages += 1;
                if header.cmsg_len >= payload_offset {
                    let declared_payload_bytes = header.cmsg_len - payload_offset;
                    let payload_bytes = declared_payload_bytes.min(available_payload_bytes);
                    malformed_rights |=
                        !declared_payload_bytes.is_multiple_of(mem::size_of::<RawFd>());
                    let fd_count = payload_bytes / mem::size_of::<RawFd>();
                    let data = data_address as *const RawFd;
                    for index in 0..fd_count {
                        let raw_fd = std::ptr::read_unaligned(data.add(index));
                        if raw_fd < 0 {
                            malformed_rights = true;
                        } else {
                            // SAFETY: each bounded nonnegative descriptor in
                            // SCM_RIGHTS was newly installed by this recvmsg call.
                            received_fds.push(OwnedFd::from_raw_fd(raw_fd));
                        }
                    }
                }
            } else {
                unexpected_control = true;
            }

            if header.cmsg_len < payload_offset || !declared_cmsg_bounded {
                break;
            }
            cmsg = CMSG_NXTHDR(&msg, cmsg);
        }
    }

    if msg.msg_flags & libc::MSG_CTRUNC != 0 {
        bail!("recvmsg ancillary data truncated (MSG_CTRUNC)");
    }
    if malformed_rights {
        bail!("malformed SCM_RIGHTS ancillary data");
    }
    if unexpected_control {
        bail!("unexpected ancillary data type");
    }
    if rights_messages != 1 || received_fds.len() != 1 {
        bail!(
            "expected exactly one SCM_RIGHTS fd, received {} in {rights_messages} message(s)",
            received_fds.len()
        );
    }

    Ok(received_fds.pop().expect("length checked above"))
}

/// Create a Unix socketpair, returning (our_end, their_end) as std UnixStreams.
pub fn create_socketpair() -> Result<(StdUnixStream, StdUnixStream)> {
    let mut fds = [-1; 2];
    // SAFETY: fds has room for both results. SOCK_CLOEXEC asks the kernel to set
    // close-on-exec atomically before either new descriptor becomes observable.
    let ret = unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
            0,
            fds.as_mut_ptr(),
        )
    };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        bail!("socketpair() failed: {err}");
    }
    // SAFETY: successful socketpair returned two distinct newly owned fds.
    let s1 = unsafe { StdUnixStream::from_raw_fd(fds[0]) };
    // SAFETY: the second descriptor is likewise newly owned and adopted once.
    let s2 = unsafe { StdUnixStream::from_raw_fd(fds[1]) };
    Ok((s1, s2))
}

#[cfg(test)]
mod tests;
