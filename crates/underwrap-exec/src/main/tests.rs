use super::*;

fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn parse_invocation_real_variant_without_args() {
    let inv = parse_invocation(&args(&["gh-authorized"])).unwrap();
    assert_eq!(inv.variant, WrapperVariant::Real);
    assert_eq!(inv.wrapper_name, "gh-authorized");
    assert!(inv.target_args.is_empty());
}

#[test]
fn parse_invocation_real_variant_passes_residual_args() {
    let inv = parse_invocation(&args(&["gh-authorized", "pr", "list"])).unwrap();
    assert_eq!(inv.variant, WrapperVariant::Real);
    assert_eq!(inv.wrapper_name, "gh-authorized");
    assert_eq!(inv.target_args, vec!["pr".to_string(), "list".to_string()]);
}

#[test]
fn parse_invocation_debug_subcommand_selects_debug_variant() {
    let inv = parse_invocation(&args(&["debug", "gh-authorized", "--version"])).unwrap();
    assert_eq!(inv.variant, WrapperVariant::Debug);
    assert_eq!(inv.wrapper_name, "gh-authorized");
    assert_eq!(inv.target_args, vec!["--version".to_string()]);
}

#[test]
fn parse_invocation_requires_a_wrapper_name() {
    let err = parse_invocation(&args(&[])).unwrap_err().to_string();
    assert!(err.contains("missing wrapper name"), "got: {err}");
}

#[test]
fn parse_invocation_debug_without_wrapper_is_an_error() {
    let err = parse_invocation(&args(&["debug"])).unwrap_err().to_string();
    assert!(
        err.contains("missing wrapper name after 'debug'"),
        "got: {err}"
    );
}

#[test]
fn parse_instance_config_minimal() {
    let cfg = parse_instance_config_text(
        r#"
socket: /run/underwrap/local.sock
"#,
    )
    .unwrap();
    assert_eq!(cfg.socket, "/run/underwrap/local.sock");
    assert!(cfg.ns_binary.is_none());
    assert!(cfg.dbus_daemon_binary.is_none());
}

#[test]
fn parse_instance_config_full() {
    let cfg = parse_instance_config_text(
        r#"
socket: /run/underwrap/local.sock
ns_binary: /run/wrappers/bin/underwrap-ns
dbus_daemon_binary: /run/current-system/sw/bin/dbus-daemon
secretsd_binary: /run/current-system/sw/bin/underwrap-secretsd
"#,
    )
    .unwrap();
    assert_eq!(
        cfg.ns_binary.as_deref(),
        Some("/run/wrappers/bin/underwrap-ns")
    );
    assert_eq!(
        cfg.secretsd_binary.as_deref(),
        Some("/run/current-system/sw/bin/underwrap-secretsd")
    );
}

#[test]
fn parse_instance_config_rejects_relative_socket() {
    let err = parse_instance_config_text("socket: run/local.sock")
        .unwrap_err()
        .to_string();
    assert!(err.contains("absolute path"), "got: {err}");
}

#[test]
fn parse_instance_config_rejects_unknown_fields() {
    let err = parse_instance_config_text(
        r#"
socket: /run/underwrap/local.sock
target: /usr/bin/gh
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("unknown field"), "got: {err}");
}

#[test]
fn relocate_resource_channel_fd_preserves_owned_socket_and_closes_source() {
    const CHILD_ENV: &str = "UNDERWRAP_EXEC_RELOCATE_CLOSE_CHILD";
    const TEST_NAME: &str =
        "tests::relocate_resource_channel_fd_preserves_owned_socket_and_closes_source";

    // A numeric EBADF witness is process-global: another parallel test can
    // reuse the source descriptor after relocation closes it. Observe it in a
    // fresh single-test process where no other test can allocate that number.
    if std::env::var(CHILD_ENV).ok().as_deref() != Some(TEST_NAME) {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .arg(TEST_NAME)
            .args(["--exact", "--nocapture", "--test-threads=1"])
            .env(CHILD_ENV, TEST_NAME)
            .output()
            .expect("exec current test executable for relocation close probe");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "relocation close probe failed: {}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
            output.status
        );
        assert!(
            stdout.contains("test result: ok. 1 passed; 0 failed;"),
            "relocation close probe did not run exactly one test:\n{stdout}"
        );
        return;
    }

    let mut fds = [-1; 2];
    // SAFETY: fds has room for both results, which are adopted exactly once below.
    assert_eq!(
        unsafe {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
                0,
                fds.as_mut_ptr(),
            )
        },
        0
    );
    // SAFETY: successful socketpair returned a newly owned descriptor.
    let source = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    // SAFETY: the peer descriptor is distinct and likewise newly owned.
    let peer = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    let source_raw = source.as_raw_fd();

    let relocated = relocate_resource_channel_fd(source).unwrap();
    assert!(relocated.as_raw_fd() >= supervision::RELOCATED_FD_MIN);
    // SAFETY: source_raw is only inspected to prove the moved source was closed.
    assert_eq!(unsafe { libc::fcntl(source_raw, libc::F_GETFD) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EBADF)
    );
    // SAFETY: relocated remains open while F_GETFD reads its descriptor flags.
    let flags = unsafe { libc::fcntl(relocated.as_raw_fd(), libc::F_GETFD) };
    assert_ne!(flags, -1);
    assert_ne!(flags & libc::FD_CLOEXEC, 0);

    let byte = [0x5a_u8];
    // SAFETY: peer is open and byte is readable for one byte.
    assert_eq!(
        unsafe { libc::write(peer.as_raw_fd(), byte.as_ptr().cast(), byte.len()) },
        1
    );
    let mut received = [0_u8; 1];
    // SAFETY: relocated is open and received is writable for one byte.
    assert_eq!(
        unsafe {
            libc::read(
                relocated.as_raw_fd(),
                received.as_mut_ptr().cast(),
                received.len(),
            )
        },
        1
    );
    assert_eq!(received, byte);
}
