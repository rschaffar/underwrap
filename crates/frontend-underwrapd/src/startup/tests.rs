use super::*;

fn minimal_config() -> DaemonConfig {
    DaemonConfig {
        backend: underwrap_protocol::config::BackendServiceConfig {
            url: "https://backend.example.internal".into(),
            ca_file: Some(std::path::PathBuf::from("/etc/underwrap/ca.pem")),
            client_cert_file: None,
            client_key_file: None,
        },
        wrappers: std::collections::HashMap::new(),
    }
}

#[test]
fn paths_overlap_detects_parent_child_and_equal() {
    assert!(paths_overlap(".config/gws", ".config/gws"));
    assert!(paths_overlap(".config/gws", ".config/gws/session"));
    assert!(paths_overlap(".config/gws/session", ".config/gws"));
    assert!(!paths_overlap(".config/gws", ".config/gws2"));
    assert!(!paths_overlap(".config/a", ".local/a"));
}

#[test]
fn validate_config_allows_unenrolled_backend_credentials() {
    validate_config(&minimal_config()).unwrap();
}

#[test]
fn validate_config_rejects_partial_backend_credentials() {
    let mut config = minimal_config();
    config.backend.client_cert_file = Some(std::path::PathBuf::from("/x/client.pem"));
    let err = validate_config(&config).expect_err("partial creds must fail");
    assert!(format!("{err:#}").contains("both be set or both be omitted"));
}
