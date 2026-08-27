use super::*;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

fn fd_is_cloexec(fd: BorrowedFd<'_>) -> bool {
    // SAFETY: fd is borrowed and open for the duration of F_GETFD.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
    assert_ne!(
        flags,
        -1,
        "F_GETFD failed: {}",
        std::io::Error::last_os_error()
    );
    flags & libc::FD_CLOEXEC != 0
}

fn pipe_pair() -> (OwnedFd, OwnedFd) {
    let mut fds = [-1; 2];
    // SAFETY: fds has room for both results, which are created atomically with
    // CLOEXEC and NONBLOCK and adopted exactly once below.
    assert_eq!(
        unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK,) },
        0,
        "pipe2 failed: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: successful pipe2 returned two distinct newly owned descriptors.
    let reader = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    // SAFETY: the second descriptor is likewise newly owned and adopted once.
    let writer = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    (reader, writer)
}

fn assert_pipe_has_no_writer(reader: BorrowedFd<'_>) {
    let mut byte = 0_u8;
    // SAFETY: reader is an open pipe read end and byte is writable for one byte.
    let read = unsafe { libc::read(reader.as_raw_fd(), (&mut byte as *mut u8).cast(), 1) };
    assert_eq!(
        read,
        0,
        "a received SCM_RIGHTS descriptor leaked; read returned {read}: {}",
        std::io::Error::last_os_error()
    );
}

/// Send several borrowed fds in one SCM_RIGHTS message.
fn send_fds(socket: BorrowedFd<'_>, fds: &[BorrowedFd<'_>]) {
    use libc::{CMSG_DATA, CMSG_FIRSTHDR, SCM_RIGHTS, SOL_SOCKET, c_void, iovec, msghdr, sendmsg};

    let data_byte: [u8; 1] = [0x42];
    let mut iov = iovec {
        iov_base: data_byte.as_ptr() as *mut c_void,
        iov_len: 1,
    };
    let raw_fds: Vec<RawFd> = fds.iter().map(AsRawFd::as_raw_fd).collect();
    let payload_bytes = mem::size_of_val(raw_fds.as_slice());
    let control_bytes = cmsg_space(payload_bytes);
    let mut control = aligned_control_storage(control_bytes);

    // SAFETY: all-zero is a valid empty msghdr; pointer fields are initialized
    // below to live storage that remains valid through sendmsg.
    let mut msg: msghdr = unsafe { mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = control_bytes;

    // SAFETY: control is cmsghdr-aligned and sized with CMSG_SPACE for every
    // raw_fds byte; all descriptors and buffers remain borrowed through sendmsg.
    unsafe {
        let cmsg = CMSG_FIRSTHDR(&msg);
        (*cmsg).cmsg_level = SOL_SOCKET;
        (*cmsg).cmsg_type = SCM_RIGHTS;
        (*cmsg).cmsg_len = cmsg_len(payload_bytes);
        std::ptr::copy_nonoverlapping(
            raw_fds.as_ptr(),
            CMSG_DATA(cmsg).cast::<RawFd>(),
            raw_fds.len(),
        );

        let ret = sendmsg(socket.as_raw_fd(), &msg, 0);
        assert!(
            ret >= 0,
            "sendmsg failed: {}",
            std::io::Error::last_os_error()
        );
    }
}

#[test]
fn control_storage_is_sized_and_aligned_for_cmsghdr() {
    for bytes in [cmsg_space(mem::size_of::<RawFd>()), receive_control_bytes()] {
        let control = aligned_control_storage(bytes);
        assert!(mem::size_of_val(control.as_slice()) >= bytes);
        assert_eq!(
            control.as_ptr() as usize % mem::align_of::<libc::cmsghdr>(),
            0
        );
    }
}

#[test]
fn socketpair_roundtrip_and_local_cloexec() {
    let (mut a, mut b) = create_socketpair().unwrap();
    assert!(fd_is_cloexec(a.as_fd()));
    assert!(fd_is_cloexec(b.as_fd()));

    a.write_all(b"hello").unwrap();
    let mut buf = [0u8; 5];
    b.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"hello");
}

#[test]
fn send_recv_fd_roundtrip_and_received_cloexec() {
    let (sender, receiver) = create_socketpair().unwrap();
    let (pipe_read, pipe_write) = pipe_pair();

    send_fd(sender.as_fd(), pipe_write.as_fd()).unwrap();
    drop(pipe_write);

    let received_fd = recv_fd(receiver.as_fd()).unwrap();
    assert!(fd_is_cloexec(received_fd.as_fd()));

    let msg = b"test_data";
    // SAFETY: received_fd is an open pipe write end and msg is readable.
    assert_eq!(
        unsafe { libc::write(received_fd.as_raw_fd(), msg.as_ptr().cast(), msg.len()) },
        msg.len() as isize
    );
    drop(received_fd);

    let mut buf = [0u8; 9];
    // SAFETY: pipe_read is open and buf is writable for its full length.
    assert_eq!(
        unsafe { libc::read(pipe_read.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len(),) },
        buf.len() as isize
    );
    assert_eq!(&buf, msg);
}

#[test]
fn recv_fd_rejects_exact_extra_and_closes_every_received_fd() {
    let (sender, receiver) = create_socketpair().unwrap();
    let (pipe_read, pipe_write) = pipe_pair();

    send_fds(sender.as_fd(), &[pipe_write.as_fd(), pipe_write.as_fd()]);
    drop(pipe_write);

    let error = recv_fd(receiver.as_fd()).unwrap_err().to_string();
    assert!(error.contains("exactly one SCM_RIGHTS fd"), "{error}");
    assert_pipe_has_no_writer(pipe_read.as_fd());
}

#[test]
fn recv_fd_rejects_truncated_ancillary_and_closes_every_received_fd() {
    let (sender, receiver) = create_socketpair().unwrap();
    let (pipe_read, pipe_write) = pipe_pair();
    let mut sent_count = 1usize;
    while cmsg_space(sent_count * mem::size_of::<RawFd>()) <= receive_control_bytes() {
        sent_count += 1;
    }
    let fds = vec![pipe_write.as_fd(); sent_count];

    send_fds(sender.as_fd(), &fds);
    drop(fds);
    drop(pipe_write);

    let error = recv_fd(receiver.as_fd()).unwrap_err().to_string();
    assert!(error.contains("MSG_CTRUNC"), "{error}");
    assert_pipe_has_no_writer(pipe_read.as_fd());
}

#[test]
fn recv_fd_rejects_mixed_credentials_and_rights_and_closes_received_fd() {
    let (sender, receiver) = create_socketpair().unwrap();
    let enabled: libc::c_int = 1;
    // SAFETY: receiver is open, and enabled is a correctly sized SO_PASSCRED value.
    assert_eq!(
        unsafe {
            libc::setsockopt(
                receiver.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PASSCRED,
                (&enabled as *const libc::c_int).cast(),
                mem::size_of_val(&enabled) as libc::socklen_t,
            )
        },
        0,
        "setsockopt(SO_PASSCRED) failed: {}",
        std::io::Error::last_os_error()
    );
    let (pipe_read, pipe_write) = pipe_pair();

    send_fd(sender.as_fd(), pipe_write.as_fd()).unwrap();
    drop(pipe_write);

    let error = recv_fd(receiver.as_fd()).unwrap_err().to_string();
    assert!(error.contains("unexpected ancillary data type"), "{error}");
    assert_pipe_has_no_writer(pipe_read.as_fd());
}
