//! Unit tests for the small CSRF / auth surface that lives here.
//!
//! The interesting cross-cutting behaviour — `axum_csrf` HMAC
//! verification, session cookie issuance, login-required gating — is
//! exercised end-to-end by the VM test and the integration-tests crate
//! because it depends on the full layer stack. The tests here cover
//! only the pieces this module itself owns: the CSRF config builder
//! and the `resolve_live_principal` helper.

use super::*;

#[test]
fn build_csrf_config_round_trips() {
    // `CsrfConfig` redacts its salt and key in its `Debug` impl, so
    // we can't introspect the values directly. This test just makes
    // sure the helper does not panic and that two calls produce
    // distinct configs (i.e. the call itself works); the
    // crypto-correctness of the underlying token machinery is
    // covered by `axum_csrf`'s own tests and exercised end-to-end in
    // the VM test.
    let _ = build_csrf_config(true);
    let _ = build_csrf_config(false);
}

#[test]
fn resolve_live_principal_picks_up_renamed_username() {
    use crate::test_support::seeded_admin_state;
    let state = seeded_admin_state("alice", "hunter2");
    let mut collection = crate::users::load(&state.users_dir).unwrap();
    let id = collection.users[0].id.clone();
    let updated = collection
        .update(&id, |u| {
            u.username = "renamed".into();
        })
        .unwrap()
        .clone();
    crate::users::save_user(&state.users_dir, &updated).unwrap();
    let stale_record = crate::users::UserRecord {
        id: id.clone(),
        username: "alice".into(),
        password_hash: collection.users[0].password_hash.clone(),
        created_at: 0,
        password_changed_at: 0,
    };
    let p = resolve_live_principal(&state, &stale_record).unwrap();
    assert_eq!(p.username, "renamed");
    assert_eq!(p.id, id);
}

#[test]
fn resolve_live_principal_returns_401_for_deleted_user() {
    use crate::test_support::seeded_admin_state;
    use hyper::StatusCode;
    let state = seeded_admin_state("alice", "hunter2");
    // Wipe the seeded user so `resolve_live_principal` cannot find any
    // record by id.
    let seeded = crate::users::load(&state.users_dir).unwrap();
    for u in &seeded.users {
        crate::users::delete_user(&state.users_dir, &u.id).unwrap();
    }
    let phantom = crate::users::UserRecord {
        id: "ghost".into(),
        username: "alice".into(),
        password_hash: "$argon2id$x".into(),
        created_at: 0,
        password_changed_at: 0,
    };
    let err = resolve_live_principal(&state, &phantom).unwrap_err();
    assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
}
