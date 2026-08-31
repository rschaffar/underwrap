use super::*;
use clap::Parser;

#[test]
fn cli_parses_config_flag() {
    let cli = Cli::parse_from(["underwrapd", "--config", "/tmp/test-config.yaml"]);
    assert_eq!(cli.config, PathBuf::from("/tmp/test-config.yaml"));
}

#[test]
fn production_subscriber_uses_text_fmt_env_filter_and_default_writer() {
    let source = include_str!("../main.rs");
    assert!(source.contains("tracing_subscriber::fmt()"));
    assert!(source.contains("tracing_subscriber::EnvFilter::try_from_default_env()"));
    assert!(source.contains("unwrap_or_else(|_| \"info\".into())"));
    assert!(!source.contains(".json()"));
    assert!(!source.contains(".with_writer("));

    let lockfile = include_str!("../../../../Cargo.lock");
    let package = lockfile
        .split("[[package]]")
        .find(|package| package.contains("name = \"tracing-subscriber\""))
        .expect("tracing-subscriber package is locked");
    assert!(package.contains("version = \"0.3.23\""), "{package}");
}

#[test]
fn parse_daemon_config_parses_minimal_yaml() {
    let config = parse_daemon_config(
        r#"
backend:
  url: https://backend.example.internal:8443
  ca_file: /etc/underwrap/ca.pem
  client_identity_file: /var/lib/underwrap-local/client-local.identity.pem
instance: local
socket: /run/underwrap/local.sock
group: underwrap-local
debug_tool_target: /run/current-system/sw/bin/underwrap-debug-tool
target_runtime_dir_base: /run/user
wrappers: {}
"#,
    )
    .unwrap();
    assert!(config.wrappers.is_empty());
    assert_eq!(config.backend.url, "https://backend.example.internal:8443");
}
