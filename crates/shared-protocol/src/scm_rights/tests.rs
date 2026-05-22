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
