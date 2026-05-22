use super::*;
use underwrap_protocol::config::BackendServiceConfig;

#[test]
fn ensure_backend_reports_missing_credential_when_files_absent() {
    let config = DaemonConfig {
        backend: BackendServiceConfig {
            url: "https://backend.example.invalid".into(),
            ca_file: None,
            client_cert_file: None,
            client_key_file: None,
        },
        wrappers: Default::default(),
    };
    let state = State::new(config);
    assert!(matches!(
        state.ensure_backend(),
        Err(BackendOpenError::MissingCredential),
    ));
    // Stays not-open: the failure is not cached.
    assert!(matches!(
        state.ensure_backend(),
        Err(BackendOpenError::MissingCredential),
    ));
}

#[test]
fn enrollment_url_points_at_client_instances_page() {
    let config = DaemonConfig {
        backend: BackendServiceConfig {
            url: "https://backend.example.invalid/".into(),
            ca_file: None,
            client_cert_file: None,
            client_key_file: None,
        },
        wrappers: Default::default(),
    };
    let state = State::new(config);
    assert_eq!(
        state.enrollment_url(),
        "https://backend.example.invalid/client-instances",
    );
}
