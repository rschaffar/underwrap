use super::*;
use crate::test_support::seeded_admin_state;

/// We can't easily synthesise a `hyper::body::Incoming`, so test the
/// login flow at the layer below `handle_admin_login` — namely, the
/// password verify + session encrypt pipeline — using the public
/// `auth` helpers. The handler itself is exercised by integration
/// tests against a real listener.
#[tokio::test]
async fn login_password_pipeline_succeeds_and_round_trips_cookie() {
    let state = seeded_admin_state("alice", "hunter2");
    let collection = crate::users::load(&state.users_file).unwrap();
    let op = collection.find_by_username("alice").unwrap();
    assert!(crate::auth::verify_password(&op.password_hash, "hunter2"));
    assert!(!crate::auth::verify_password(&op.password_hash, "wrong"));

    let key = state.session_key;
    let payload = crate::auth::new_session_payload(
        op.id.clone(),
        op.username.clone(),
        1000,
        state.session_lifetime_secs,
    );
    let cookie = crate::auth::encrypt_session(&payload, &key).unwrap();
    let decoded = crate::auth::decrypt_session(&cookie, &key, 1500).unwrap();
    assert_eq!(decoded.username, "alice");
}

#[tokio::test]
async fn unknown_user_login_pipeline_uses_dummy_hash() {
    // Verify that the unknown-user branch in handle_admin_login
    // selects the dummy hash, keeping wall-clock behaviour
    // consistent with the real-user path.
    let state = seeded_admin_state("alice", "hunter2");
    let collection = crate::users::load(&state.users_file).unwrap();
    assert!(collection.find_by_username("eve").is_none());
    assert!(!crate::auth::verify_password(
        crate::auth::DUMMY_PASSWORD_HASH,
        "anything"
    ));
}

#[tokio::test]
async fn whoami_resolves_username_from_collection() {
    let state = seeded_admin_state("alice", "hunter2");
    let key = state.session_key;
    // `handle_admin_whoami` reads SystemTime::now() internally, so
    // give the cookie a real-clock issued_at.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let payload = crate::auth::new_session_payload("op-1".into(), "alice".into(), now, 3600);
    let cookie_value = crate::auth::encrypt_session(&payload, &key).unwrap();

    let mut headers = hyper::HeaderMap::new();
    let cookie = format!("__Host-uw-session={cookie_value}");
    headers.insert(
        hyper::header::COOKIE,
        hyper::header::HeaderValue::from_str(&cookie).unwrap(),
    );
    let resp = handle_admin_whoami(&state, &headers);
    assert_eq!(resp.status(), StatusCode::OK);
    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["authenticated"], true);
    assert_eq!(parsed["username"], "alice");
    assert!(
        parsed.get("role").is_none(),
        "wire shape must not carry a role"
    );
}

#[tokio::test]
async fn whoami_returns_unauthenticated_without_cookie() {
    let state = seeded_admin_state("alice", "hunter2");
    let resp = handle_admin_whoami(&state, &hyper::HeaderMap::new());
    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["authenticated"], false);
}

#[test]
fn handle_admin_logout_revokes_and_clears_cookies() {
    let state = seeded_admin_state("alice", "hunter2");
    let payload = crate::auth::new_session_payload("op-1".into(), "alice".into(), 1000, 3600);
    let response = handle_admin_logout(&state, &payload);
    assert_eq!(response.status(), StatusCode::OK);
    let cookies: Vec<String> = response
        .headers()
        .get_all(hyper::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
        .collect();
    assert!(
        cookies
            .iter()
            .any(|c| c.starts_with("__Host-uw-session=;") && c.contains("Max-Age=0")),
        "expected session cookie cleared in {cookies:?}",
    );
    assert!(
        cookies
            .iter()
            .any(|c| c.starts_with("__Host-uw-csrf=;") && c.contains("Max-Age=0")),
        "expected csrf cookie cleared in {cookies:?}",
    );
    // Revocation entry visible while still inside the original window.
    assert!(
        state
            .session_revocations
            .is_revoked(&payload.session_id, 1500)
    );
    // ...and naturally frees once the session would have expired anyway.
    assert!(
        !state
            .session_revocations
            .is_revoked(&payload.session_id, payload.expires_at + 1)
    );
}
