use std::sync::Arc;

use axum::extract::State;
use hyper::StatusCode;

use super::*;
use crate::test_support::{fresh_bootstrap_state, run_bootstrap, run_handler};

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_status_required_on_fresh_store() {
    let state = Arc::new(fresh_bootstrap_state());
    let (status, body) = run_handler(handle_admin_bootstrap_status(State(state)));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["bootstrap_required"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_status_false_when_sentinel_set() {
    let state = Arc::new(fresh_bootstrap_state());
    crate::users::mark_bootstrap_completed(&state.admin().bootstrap_file).unwrap();
    let (status, body) = run_handler(handle_admin_bootstrap_status(State(state)));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["bootstrap_required"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_status_false_when_operators_exist_even_without_sentinel() {
    let state = Arc::new(fresh_bootstrap_state());
    let record = crate::users::UserRecord {
        id: crate::users::new_user_id(),
        username: "alice".into(),
        password_hash: crate::auth::hash_password("p").unwrap(),
        created_at: 1000,
        password_changed_at: 1000,
    };
    let mut collection = crate::users::UserCollection::default();
    collection.add(record.clone()).unwrap();
    crate::users::save_user(&state.admin().users_dir, &record).unwrap();
    let (_, body) = run_handler(handle_admin_bootstrap_status(State(state)));
    assert_eq!(body["bootstrap_required"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_succeeds_on_empty_store_and_sets_sentinel() {
    let state = fresh_bootstrap_state();
    let (status, _) = run_bootstrap(&state, "alice", "hunter2-long");
    assert_eq!(status, StatusCode::OK);
    // Sentinel is set.
    assert!(crate::users::bootstrap_completed(&state.admin().bootstrap_file).unwrap());
    // First user record is created.
    let collection = crate::users::load(&state.admin().users_dir).unwrap();
    let u = collection.find_by_username("alice").unwrap();
    assert!(crate::auth::verify_password(
        &u.password_hash,
        "hunter2-long"
    ));
    // Session and CSRF cookies are emitted by the
    // `tower-sessions` / `axum-login` / `axum_csrf` layers, not by
    // the bootstrap handler itself. The integration tests cover the
    // full layered response; here we just confirm the unit-level DB
    // side-effects.
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_second_call_returns_bootstrap_already_completed() {
    let state = fresh_bootstrap_state();
    let _ = run_bootstrap(&state, "alice", "hunter2-long");
    let (status, body) = run_bootstrap(&state, "eve", "haxx0r");
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "bootstrap_already_completed");
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_refuses_when_users_exist_without_sentinel() {
    // This is the "CLI recovery happened first" case: a user
    // exists, but the sentinel was never set. The online flow
    // must still refuse with users_already_exist.
    let state = fresh_bootstrap_state();
    let record = crate::users::UserRecord {
        id: crate::users::new_user_id(),
        username: "rescuer".into(),
        password_hash: crate::auth::hash_password("rescue").unwrap(),
        created_at: 1000,
        password_changed_at: 1000,
    };
    let mut collection = crate::users::UserCollection::default();
    collection.add(record.clone()).unwrap();
    crate::users::save_user(&state.admin().users_dir, &record).unwrap();

    let (status, body) = run_bootstrap(&state, "newbie", "x");
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "users_already_exist");
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_concurrent_calls_only_one_succeeds() {
    let state = Arc::new(fresh_bootstrap_state());
    let s1 = state.clone();
    let s2 = state.clone();
    let h1 = tokio::task::spawn_blocking(move || run_bootstrap(&s1, "alice", "password-one"));
    let h2 = tokio::task::spawn_blocking(move || run_bootstrap(&s2, "bob", "password-two"));
    let (st1, _) = h1.await.unwrap();
    let (st2, _) = h2.await.unwrap();
    let statuses = [st1, st2];
    let ok_count = statuses.iter().filter(|s| **s == StatusCode::OK).count();
    let conflict_count = statuses
        .iter()
        .filter(|s| **s == StatusCode::CONFLICT)
        .count();
    assert_eq!(ok_count, 1, "exactly one bootstrap must win: {statuses:?}");
    assert_eq!(conflict_count, 1, "the loser must see 409: {statuses:?}");
    // Sentinel is set and collection has exactly one operator.
    assert!(crate::users::bootstrap_completed(&state.admin().bootstrap_file).unwrap());
    let collection = crate::users::load(&state.admin().users_dir).unwrap();
    assert_eq!(collection.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_refuses_empty_password() {
    let state = fresh_bootstrap_state();
    let (status, body) = run_bootstrap(&state, "alice", "");
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "password_empty");
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_refuses_short_password() {
    let state = fresh_bootstrap_state();
    let short = "a".repeat(crate::auth::MIN_PASSWORD_LEN - 1);
    let (status, body) = run_bootstrap(&state, "alice", &short);
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "password_too_short");
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_accepts_min_length_password() {
    let state = fresh_bootstrap_state();
    let min = "a".repeat(crate::auth::MIN_PASSWORD_LEN);
    let (status, _) = run_bootstrap(&state, "alice", &min);
    assert_eq!(status, StatusCode::OK);
}
