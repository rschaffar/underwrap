//! Unit tests for the password-verify + login-pipeline helpers.
//!
//! The HTTP-side handlers (`handle_admin_login`, `handle_admin_logout`,
//! `handle_admin_whoami`, `handle_admin_password_change`) all take an
//! `AuthSession<AuthBackend>` which is produced by the
//! `tower-sessions` + `axum-login` middleware stack and is not
//! constructable in isolation here. End-to-end coverage for those
//! handlers lives in the integration-tests crate and the VM test. The
//! tests below exercise the password-verify primitives the handlers
//! lean on.

use crate::auth::DUMMY_PASSWORD_HASH;
use crate::test_support::seeded_admin_state;

#[test]
fn seeded_user_password_round_trips() {
    let state = seeded_admin_state("alice", "hunter2");
    let collection = crate::users::load(&state.users_file).unwrap();
    let op = collection.find_by_username("alice").unwrap();
    assert!(crate::auth::verify_password(&op.password_hash, "hunter2"));
    assert!(!crate::auth::verify_password(&op.password_hash, "wrong"));
}

#[test]
fn unknown_user_dummy_hash_never_matches() {
    let state = seeded_admin_state("alice", "hunter2");
    let collection = crate::users::load(&state.users_file).unwrap();
    assert!(collection.find_by_username("eve").is_none());
    assert!(!crate::auth::verify_password(
        DUMMY_PASSWORD_HASH,
        "anything"
    ));
}
