//! SCM_RIGHTS helpers for passing file descriptors over Unix sockets.

use anyhow::{Result, bail};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::net::UnixStream as StdUnixStream;

/// Send a file descriptor over a Unix stream socket using SCM_RIGHTS.
///
/// Also sends a single data byte (required — ancillary data is only
/// delivered alongside actual data on stream sockets).
pub fn send_fd(socket: &StdUnixStream, fd: RawFd) -> Result<()> {
    use libc::{
        CMSG_DATA, CMSG_FIRSTHDR, CMSG_LEN, CMSG_SPACE, SCM_RIGHTS, SOL_SOCKET, c_void, iovec,
        msghdr, sendmsg,
    };
    use std::mem;
    use std::ptr;

    let data_byte: [u8; 1] = [0x42]; // Arbitrary marker byte.
    let mut iov = iovec {
        iov_base: data_byte.as_ptr() as *mut c_void,
        iov_len: 1,
    };

    // Ancillary buffer for one fd.
    let cmsg_space = unsafe { CMSG_SPACE(mem::size_of::<RawFd>() as u32) } as usize;
    let mut cmsg_buf = vec![0u8; cmsg_space];

    let mut msg: msghdr = unsafe { mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg_buf.as_mut_ptr() as *mut c_void;
    msg.msg_controllen = cmsg_space;

    unsafe {
        let cmsg: *mut libc::cmsghdr = CMSG_FIRSTHDR(&msg);
        (*cmsg).cmsg_level = SOL_SOCKET;
        (*cmsg).cmsg_type = SCM_RIGHTS;
        (*cmsg).cmsg_len = CMSG_LEN(mem::size_of::<RawFd>() as u32) as usize;
        ptr::copy_nonoverlapping(&fd as *const RawFd, CMSG_DATA(cmsg) as *mut RawFd, 1);

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
/// Returns the received fd and the data byte.
pub fn recv_fd(socket: &StdUnixStream) -> Result<RawFd> {
    use libc::{
        CMSG_DATA, CMSG_FIRSTHDR, CMSG_SPACE, SCM_RIGHTS, SOL_SOCKET, c_void, iovec, msghdr,
        recvmsg,
    };
    use std::mem;

    let mut data_byte: [u8; 1] = [0];
    let mut iov = iovec {
        iov_base: data_byte.as_mut_ptr() as *mut c_void,
        iov_len: 1,
    };

    let cmsg_space = unsafe { CMSG_SPACE(mem::size_of::<RawFd>() as u32) } as usize;
    let mut cmsg_buf = vec![0u8; cmsg_space];

    let mut msg: msghdr = unsafe { mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg_buf.as_mut_ptr() as *mut c_void;
    msg.msg_controllen = cmsg_space;

    unsafe {
        let ret = recvmsg(socket.as_raw_fd(), &mut msg, 0);
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            bail!("recvmsg(SCM_RIGHTS) failed: {err}");
        }
        if ret == 0 {
            bail!("recvmsg: connection closed");
        }

        let cmsg = CMSG_FIRSTHDR(&msg);
        if cmsg.is_null() {
            bail!("no ancillary data received");
        }
        if (*cmsg).cmsg_level != SOL_SOCKET || (*cmsg).cmsg_type != SCM_RIGHTS {
            bail!("unexpected ancillary data type");
        }

        let mut fd: RawFd = -1;
        std::ptr::copy_nonoverlapping(CMSG_DATA(cmsg) as *const RawFd, &mut fd, 1);

        if fd < 0 {
            bail!("received invalid fd");
        }

        Ok(fd)
    }
}

/// Create a Unix socketpair, returning (our_end, their_end) as std UnixStreams.
pub fn create_socketpair() -> Result<(StdUnixStream, StdUnixStream)> {
    let mut fds = [0i32; 2];
    let ret = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        bail!("socketpair() failed: {err}");
    }
    let s1 = unsafe { StdUnixStream::from_raw_fd(fds[0]) };
    let s2 = unsafe { StdUnixStream::from_raw_fd(fds[1]) };
    Ok((s1, s2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn socketpair_roundtrip() {
        let (mut a, mut b) = create_socketpair().unwrap();
        a.write_all(b"hello").unwrap();
        let mut buf = [0u8; 5];
        b.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"hello");
    }

    #[test]
    fn send_recv_fd() {
        // Create a socketpair for fd passing.
        let (sender, receiver) = create_socketpair().unwrap();

        // Create a pipe — we'll pass the write end.
        let mut pipe_fds = [0i32; 2];
        unsafe { libc::pipe(pipe_fds.as_mut_ptr()) };
        let pipe_read = pipe_fds[0];
        let pipe_write = pipe_fds[1];

        // Send the write end.
        send_fd(&sender, pipe_write).unwrap();

        // Close our copy of the write end.
        unsafe { libc::close(pipe_write) };

        // Receive the write end in the receiver.
        let received_fd = recv_fd(&receiver).unwrap();

        // Write through the received fd.
        unsafe {
            let msg = b"test_data";
            libc::write(received_fd, msg.as_ptr() as *const _, msg.len());
            libc::close(received_fd);
        }

        // Read from the pipe read end.
        let mut buf = [0u8; 9];
        unsafe {
            libc::read(pipe_read, buf.as_mut_ptr() as *mut _, buf.len());
            libc::close(pipe_read);
        }
        assert_eq!(&buf, b"test_data");
    }
}
