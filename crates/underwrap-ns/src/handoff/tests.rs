use super::*;

fn current_identity(account: &str) -> DaemonIdentity {
    DaemonIdentity {
        uid: unsafe { libc::getuid() },
        gid: unsafe { libc::getgid() },
        account: account.to_string(),
    }
}

fn socketpair(socket_type: i32) -> [i32; 2] {
    let mut fds = [-1; 2];
    assert_eq!(
        unsafe { libc::socketpair(libc::AF_UNIX, socket_type, 0, fds.as_mut_ptr()) },
        0
    );
    fds
}

fn close_pair(fds: [i32; 2]) {
    unsafe {
        libc::close(fds[0]);
        libc::close(fds[1]);
    }
}

#[test]
fn stream_socketpair_accepts_its_creation_credentials() {
    let fds = socketpair(libc::SOCK_STREAM);
    authenticate_channel_fd(fds[0], &current_identity("self")).unwrap();
    close_pair(fds);
}

#[test]
fn stream_socketpair_rejects_wrong_uid() {
    let fds = socketpair(libc::SOCK_STREAM);
    let mut expected = current_identity("wrong-uid");
    expected.uid = if expected.uid == 0 { 1 } else { 0 };
    let err = authenticate_channel_fd(fds[0], &expected).unwrap_err();
    assert!(
        err.contains("does not match selected instance daemon"),
        "{err}"
    );
    close_pair(fds);
}

#[test]
fn stream_socketpair_rejects_wrong_gid() {
    let fds = socketpair(libc::SOCK_STREAM);
    let mut expected = current_identity("wrong-gid");
    expected.gid = if expected.gid == 0 { 1 } else { 0 };
    let err = authenticate_channel_fd(fds[0], &expected).unwrap_err();
    assert!(
        err.contains("does not match selected instance daemon"),
        "{err}"
    );
    close_pair(fds);
}

#[test]
fn datagram_socketpair_is_rejected() {
    let fds = socketpair(libc::SOCK_DGRAM);
    let err = authenticate_channel_fd(fds[0], &current_identity("self")).unwrap_err();
    assert!(err.contains("expected SOCK_STREAM"), "{err}");
    close_pair(fds);
}

#[test]
fn pipe_is_rejected() {
    let mut fds = [-1; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    let err = authenticate_channel_fd(fds[0], &current_identity("self")).unwrap_err();
    assert!(err.contains("SO_DOMAIN"), "{err}");
    close_pair(fds);
}

#[test]
fn invalid_fd_is_rejected() {
    let err = authenticate_channel_fd(-1, &current_identity("self")).unwrap_err();
    assert!(err.contains("SO_DOMAIN"), "{err}");
}

#[test]
fn missing_daemon_account_is_rejected() {
    let err = resolve_daemon_identity("this-instance-must-not-exist-7f4a2d").unwrap_err();
    assert!(err.contains("does not exist"), "{err}");
}

#[test]
fn daemon_and_target_identity_must_be_distinct() {
    let daemon = DaemonIdentity {
        uid: 100,
        gid: 200,
        account: "underwrap-test".to_string(),
    };
    require_distinct_target(&daemon, 100, 300).unwrap_err();
    require_distinct_target(&daemon, 300, 200).unwrap_err();
    require_distinct_target(&daemon, 300, 400).unwrap();
}
