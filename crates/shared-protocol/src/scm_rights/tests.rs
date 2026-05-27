use super::*;
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;

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

/// Send several fds in one `sendmsg`. The receiver's ancillary buffer is
/// sized for a single fd; once the sender attaches more fds than fit
/// (accounting for cmsg alignment padding) the kernel sets `MSG_CTRUNC` and
/// `recv_fd` must reject rather than silently keep only the fds that fit.
fn send_fds(socket: &StdUnixStream, fds: &[RawFd]) {
    use libc::{
        CMSG_DATA, CMSG_FIRSTHDR, CMSG_LEN, CMSG_SPACE, SCM_RIGHTS, SOL_SOCKET, c_void, iovec,
        msghdr, sendmsg,
    };
    use std::mem;

    let data_byte: [u8; 1] = [0x42];
    let mut iov = iovec {
        iov_base: data_byte.as_ptr() as *mut c_void,
        iov_len: 1,
    };

    let payload = mem::size_of_val(fds) as u32;
    let cmsg_space = unsafe { CMSG_SPACE(payload) } as usize;
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
        (*cmsg).cmsg_len = CMSG_LEN(payload) as usize;
        std::ptr::copy_nonoverlapping(fds.as_ptr(), CMSG_DATA(cmsg) as *mut RawFd, fds.len());

        let ret = sendmsg(socket.as_raw_fd(), &msg, 0);
        assert!(
            ret >= 0,
            "sendmsg failed: {}",
            std::io::Error::last_os_error()
        );
    }
}

#[test]
fn recv_fd_rejects_truncated_ancillary() {
    let (sender, receiver) = create_socketpair().unwrap();

    // A one-fd ancillary buffer has alignment padding that happens to hold
    // two fds, so send three to guarantee truncation.
    let mut pipe_fds = [0i32; 2];
    unsafe { libc::pipe(pipe_fds.as_mut_ptr()) };
    let dup_fd = unsafe { libc::dup(pipe_fds[0]) };

    send_fds(&sender, &[pipe_fds[0], pipe_fds[1], dup_fd]);

    let result = recv_fd(&receiver);
    assert!(
        result.is_err(),
        "recv_fd must reject a message whose ancillary data was truncated"
    );

    unsafe {
        libc::close(pipe_fds[0]);
        libc::close(pipe_fds[1]);
        libc::close(dup_fd);
    }
}
