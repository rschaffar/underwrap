use super::*;
use crate::test_support::headers_with;
use hyper::Method;

fn cfg_with_origins(origins: Vec<String>) -> AdminConfig {
    AdminConfig {
        allowed_origins: origins,
        session_lifetime_secs: crate::auth::DEFAULT_SESSION_LIFETIME_SECS,
        stores_data_root: None,
    }
}

#[test]
fn validate_admin_config_allows_nonempty_origin_entries() {
    let cfg = cfg_with_origins(vec!["https://ok.example".to_string()]);
    validate_admin_config(&cfg).expect("nonempty origins must be allowed");
}

#[test]
fn validate_admin_config_rejects_empty_origin_entry() {
    let cfg = cfg_with_origins(vec!["https://ok.example".to_string(), "  ".to_string()]);
    let err = validate_admin_config(&cfg).unwrap_err();
    assert!(format!("{err:#}").contains("empty entry"));
}

#[test]
fn check_admin_origin_passes_get_without_check() {
    let allowed = vec!["https://ok.example".to_string()];
    assert!(check_admin_origin(&Method::GET, &headers_with(&[]), &allowed).is_ok());
    assert!(check_admin_origin(&Method::HEAD, &headers_with(&[]), &allowed).is_ok());
    assert!(check_admin_origin(&Method::OPTIONS, &headers_with(&[]), &allowed).is_ok());
}

#[test]
fn check_admin_origin_passes_when_no_allowlist_configured() {
    // Empty allowlist == no check, even for POST.
    assert!(check_admin_origin(&Method::POST, &headers_with(&[]), &[]).is_ok());
}

#[test]
fn check_admin_origin_accepts_matching_origin() {
    let allowed = vec!["https://ok.example".to_string()];
    let headers = headers_with(&[("Origin", "https://ok.example")]);
    assert!(check_admin_origin(&Method::POST, &headers, &allowed).is_ok());
}

#[test]
fn check_admin_origin_rejects_mismatched_origin() {
    let allowed = vec!["https://ok.example".to_string()];
    let headers = headers_with(&[("Origin", "https://attacker.example")]);
    let err = check_admin_origin(&Method::POST, &headers, &allowed).unwrap_err();
    assert!(err.contains("not in allowed_origins"));
}

#[test]
fn check_admin_origin_falls_back_to_referer() {
    let allowed = vec!["https://ok.example".to_string()];
    let headers = headers_with(&[("Referer", "https://ok.example/admin/users")]);
    assert!(check_admin_origin(&Method::POST, &headers, &allowed).is_ok());
}

#[test]
fn check_admin_origin_rejects_referer_from_other_origin() {
    let allowed = vec!["https://ok.example".to_string()];
    let headers = headers_with(&[("Referer", "https://attacker.example/page")]);
    let err = check_admin_origin(&Method::POST, &headers, &allowed).unwrap_err();
    assert!(err.contains("not in allowed_origins"));
}

#[test]
fn check_admin_origin_rejects_when_origin_and_referer_missing() {
    let allowed = vec!["https://ok.example".to_string()];
    let err = check_admin_origin(&Method::POST, &headers_with(&[]), &allowed).unwrap_err();
    assert!(err.contains("missing Origin"));
}

#[test]
fn referer_to_origin_strips_path_and_query() {
    assert_eq!(
        referer_to_origin("https://ok.example/foo/bar?x=1#y").as_deref(),
        Some("https://ok.example")
    );
    assert_eq!(
        referer_to_origin("http://localhost:5173/").as_deref(),
        Some("http://localhost:5173")
    );
}

#[test]
fn referer_to_origin_rejects_malformed_input() {
    assert!(referer_to_origin("not-a-url").is_none());
    assert!(referer_to_origin("https://").is_none());
    assert!(referer_to_origin("://nohost").is_none());
}
