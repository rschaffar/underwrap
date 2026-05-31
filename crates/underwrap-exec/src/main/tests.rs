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
