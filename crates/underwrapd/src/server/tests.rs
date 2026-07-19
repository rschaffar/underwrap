use super::*;
use crate::test_support::config_with_backend_url;

#[test]
fn ensure_backend_reports_missing_credential_when_files_absent() {
    let config = config_with_backend_url("https://backend.example.invalid");
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
    let config = config_with_backend_url("https://backend.example.invalid/");
    let state = State::new(config);
    assert_eq!(
        state.enrollment_url(),
        "https://backend.example.invalid/client-instances",
    );
}

#[test]
fn format_register_pairs_renders_wrapper_arrow_resource_csv() {
    let pairs = vec![
        RuntimeWrapperRegisterPair {
            wrapper_name: "gh".into(),
            resource_name: "github/token".into(),
        },
        RuntimeWrapperRegisterPair {
            wrapper_name: "jira".into(),
            resource_name: "atlassian/api".into(),
        },
    ];
    assert_eq!(
        format_register_pairs(&pairs),
        "gh=>github/token, jira=>atlassian/api",
    );
}

#[test]
fn format_register_pairs_empty_is_empty_string() {
    assert_eq!(format_register_pairs(&[]), "");
}
