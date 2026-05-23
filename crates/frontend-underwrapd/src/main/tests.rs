use super::*;
use clap::Parser;

#[test]
fn cli_parses_config_flag() {
    let cli = Cli::parse_from(["underwrapd", "--config", "/tmp/test-config.yaml"]);
    assert_eq!(cli.config, PathBuf::from("/tmp/test-config.yaml"));
}

#[test]
fn parse_daemon_config_parses_minimal_yaml() {
    let config = parse_daemon_config(
        r#"
backend:
  url: https://backend.example.internal:8443
  ca_file: /etc/underwrap/ca.pem
  client_cert_file: /etc/underwrap/client.pem
  client_key_file: /etc/underwrap/client.key
wrappers: {}
"#,
    )
    .unwrap();
    assert!(config.wrappers.is_empty());
    assert_eq!(config.backend.url, "https://backend.example.internal:8443");
}
