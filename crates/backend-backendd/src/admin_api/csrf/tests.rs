use super::*;
use crate::http::responses::json_response;
use crate::test_support::{headers_with_session_cookie, seeded_admin_state, session_headers};
use hyper::Method;

#[test]
fn authenticate_admin_rejects_missing_and_invalid_session_cookie() {
    let state = seeded_admin_state("alice", "hunter2");
    assert!(authenticate_admin(&state, &headers_with_session_cookie(None)).is_err());
    assert!(
        authenticate_admin(&state, &headers_with_session_cookie(Some("not-a-session"))).is_err()
    );
}

#[test]
fn resolve_principal_session_resolves_username_from_collection() {
    let state = seeded_admin_state("alice", "hunter2");
    let users_file = state.users_file.clone();
    let mut collection = crate::users::UserCollection::default();
    let hash = crate::auth::hash_password("hunter2").unwrap();
    collection
        .add(crate::users::UserRecord {
            id: crate::users::new_user_id(),
            username: "alice".into(),
            password_hash: hash,
            created_at: 1000,
            password_changed_at: 1000,
        })
        .unwrap();
    crate::users::save(&users_file, &collection).unwrap();

    let payload = crate::auth::new_session_payload("op-1".into(), "alice".into(), 1000, 3600);
    let auth = AdminAuth(payload);
    let p = resolve_principal(&state, &auth).unwrap();
    assert_eq!(p.username, "alice");
}

#[test]
fn resolve_principal_session_fails_when_user_was_deleted() {
    // Cookie says "alice" but the collection has no record of her.
    let state = seeded_admin_state("alice", "hunter2");
    // Save an empty collection over alice's slot.
    crate::users::save(&state.users_file, &crate::users::UserCollection::default()).unwrap();
    let payload = crate::auth::new_session_payload("op-1".into(), "alice".into(), 1000, 3600);
    let auth = AdminAuth(payload);
    match resolve_principal(&state, &auth) {
        Ok(_) => panic!("expected 401 for deleted operator"),
        Err(resp) => assert_eq!(resp.status(), StatusCode::UNAUTHORIZED),
    }
}

#[test]
fn authenticate_admin_rejects_revoked_session() {
    let state = seeded_admin_state("alice", "hunter2");
    let (headers, payload) = session_headers(&state, "alice", false, None);
    let now = crate::users::now_epoch_secs();
    state
        .session_revocations
        .revoke(payload.session_id.clone(), payload.expires_at, now);
    let err = authenticate_admin(&state, &headers).unwrap_err();
    assert!(matches!(err, AdminAuthError::Revoked));
}

#[test]
fn authenticate_admin_accepts_session_when_not_revoked() {
    let state = seeded_admin_state("alice", "hunter2");
    let (headers, _payload) = session_headers(&state, "alice", false, None);
    let auth = authenticate_admin(&state, &headers).unwrap();
    assert_eq!(auth.operator(), "alice");
    assert_eq!(auth.session().username, "alice");
}

#[test]
fn attach_csrf_cookie_refreshes_on_session_auth() {
    let state = seeded_admin_state("alice", "hunter2");
    let payload = crate::auth::new_session_payload("op-1".into(), "alice".into(), 1000, 3600);
    let response = json_response(StatusCode::OK, &serde_json::json!({"hello": "world"}));
    let wrapped = attach_csrf_cookie(response, &state, &AdminAuth(payload.clone()));
    let cookies: Vec<String> = wrapped
        .headers()
        .get_all(hyper::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
        .collect();
    assert!(
        cookies
            .iter()
            .any(|c| c.starts_with(&format!("__Host-uw-csrf={}", payload.csrf_secret))),
        "expected refreshed CSRF cookie in {cookies:?}",
    );
}

#[test]
fn check_csrf_passes_safe_methods_without_token() {
    let headers = hyper::HeaderMap::new();
    check_csrf(&Method::GET, &headers, "any-secret").unwrap();
    check_csrf(&Method::HEAD, &headers, "any-secret").unwrap();
    check_csrf(&Method::OPTIONS, &headers, "any-secret").unwrap();
}

#[test]
fn check_csrf_rejects_missing_header_with_csrf_required() {
    let mut headers = hyper::HeaderMap::new();
    headers.insert(
        hyper::header::COOKIE,
        hyper::header::HeaderValue::from_static("__Host-uw-csrf=the-secret"),
    );
    let (code, _msg) = check_csrf(&Method::POST, &headers, "the-secret").unwrap_err();
    assert_eq!(code, "csrf_required");
}

#[test]
fn check_csrf_rejects_missing_cookie_with_csrf_required() {
    let mut headers = hyper::HeaderMap::new();
    headers.insert(
        hyper::header::HeaderName::from_static("x-csrf-token"),
        hyper::header::HeaderValue::from_static("the-secret"),
    );
    let (code, _msg) = check_csrf(&Method::POST, &headers, "the-secret").unwrap_err();
    assert_eq!(code, "csrf_required");
}

#[test]
fn check_csrf_rejects_mismatched_header_with_csrf_mismatch() {
    let mut headers = hyper::HeaderMap::new();
    headers.insert(
        hyper::header::COOKIE,
        hyper::header::HeaderValue::from_static("__Host-uw-csrf=the-secret"),
    );
    headers.insert(
        hyper::header::HeaderName::from_static("x-csrf-token"),
        hyper::header::HeaderValue::from_static("a-different-value"),
    );
    let (code, _) = check_csrf(&Method::POST, &headers, "the-secret").unwrap_err();
    assert_eq!(code, "csrf_mismatch");
}

#[test]
fn check_csrf_rejects_when_cookie_does_not_match_session() {
    // Same value in cookie and header, but neither matches the
    // session's csrf_secret — a cookie-tossing or stale-cookie
    // scenario must not slip through.
    let mut headers = hyper::HeaderMap::new();
    headers.insert(
        hyper::header::COOKIE,
        hyper::header::HeaderValue::from_static("__Host-uw-csrf=stale-value"),
    );
    headers.insert(
        hyper::header::HeaderName::from_static("x-csrf-token"),
        hyper::header::HeaderValue::from_static("stale-value"),
    );
    let (code, _) = check_csrf(&Method::POST, &headers, "real-session-secret").unwrap_err();
    assert_eq!(code, "csrf_mismatch");
}

#[test]
fn check_csrf_accepts_matching_header_cookie_and_session() {
    let mut headers = hyper::HeaderMap::new();
    headers.insert(
        hyper::header::COOKIE,
        hyper::header::HeaderValue::from_static("__Host-uw-csrf=the-secret"),
    );
    headers.insert(
        hyper::header::HeaderName::from_static("x-csrf-token"),
        hyper::header::HeaderValue::from_static("the-secret"),
    );
    check_csrf(&Method::POST, &headers, "the-secret").unwrap();
}
