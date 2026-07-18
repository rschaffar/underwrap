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
fn relocate_resource_channel_fd_preserves_socket_and_closes_source() {
    let mut fds = [-1; 2];
    assert_eq!(
        unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) },
        0
    );
    let source = fds[0];
    let peer = fds[1];

    let relocated = relocate_resource_channel_fd(source).unwrap();
    assert!(relocated >= 21);
    assert_eq!(unsafe { libc::fcntl(source, libc::F_GETFD) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EBADF)
    );
    let flags = unsafe { libc::fcntl(relocated, libc::F_GETFD) };
    assert_ne!(flags, -1);
    assert_ne!(flags & libc::FD_CLOEXEC, 0);

    let byte = [0x5a_u8];
    assert_eq!(
        unsafe { libc::write(peer, byte.as_ptr().cast(), byte.len()) },
        1
    );
    let mut received = [0_u8; 1];
    assert_eq!(
        unsafe { libc::read(relocated, received.as_mut_ptr().cast(), received.len()) },
        1
    );
    assert_eq!(received, byte);

    unsafe {
        libc::close(relocated);
        libc::close(peer);
    }
}

#[test]
fn relocate_resource_channel_fd_rejects_invalid_source() {
    let err = relocate_resource_channel_fd(-1).unwrap_err().to_string();
    assert!(err.contains("relocating wrapper-resource channel"), "{err}");
}
