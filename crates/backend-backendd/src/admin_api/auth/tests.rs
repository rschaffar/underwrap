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
    let collection = crate::users::load(&state.admin().users_dir).unwrap();
    let op = collection.find_by_username("alice").unwrap();
    assert!(crate::auth::verify_password(&op.password_hash, "hunter2"));
    assert!(!crate::auth::verify_password(&op.password_hash, "wrong"));
}

#[test]
fn unknown_user_dummy_hash_never_matches() {
    let state = seeded_admin_state("alice", "hunter2");
    let collection = crate::users::load(&state.admin().users_dir).unwrap();
    assert!(collection.find_by_username("eve").is_none());
    assert!(!crate::auth::verify_password(
        DUMMY_PASSWORD_HASH,
        "anything"
    ));
}

#[test]
fn validate_password_rejects_empty() {
    assert_eq!(crate::auth::validate_password(""), Err("password_empty"));
}

#[test]
fn validate_password_rejects_under_minimum() {
    let short = "a".repeat(crate::auth::MIN_PASSWORD_LEN - 1);
    assert_eq!(
        crate::auth::validate_password(&short),
        Err("password_too_short"),
    );
}

#[test]
fn validate_password_accepts_minimum() {
    let min = "a".repeat(crate::auth::MIN_PASSWORD_LEN);
    assert!(crate::auth::validate_password(&min).is_ok());
}

#[test]
fn validate_password_counts_chars_not_bytes() {
    // 12 four-byte UTF-8 characters (48 bytes total) is at minimum length.
    let twelve_chars = "\u{1F600}".repeat(crate::auth::MIN_PASSWORD_LEN);
    assert!(crate::auth::validate_password(&twelve_chars).is_ok());
}
