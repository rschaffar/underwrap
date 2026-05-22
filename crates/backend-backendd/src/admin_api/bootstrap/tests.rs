use super::*;
use crate::test_support::{fresh_bootstrap_state, read_json, run_bootstrap};
use std::sync::Arc;

#[tokio::test]
async fn bootstrap_status_required_on_fresh_store() {
    let state = fresh_bootstrap_state();
    let resp = handle_admin_bootstrap_status(&state);
    assert_eq!(resp.status(), StatusCode::OK);
    let body = read_json(resp).await;
    assert_eq!(body["bootstrap_required"], true);
}

#[tokio::test]
async fn bootstrap_status_false_when_sentinel_set() {
    let state = fresh_bootstrap_state();
    crate::users::mark_bootstrap_completed(
        &state.users_file,
        &crate::users::BootstrapSentinel {
            completed_at: 1000,
            by_username: "alice".into(),
        },
    )
    .unwrap();
    let body = read_json(handle_admin_bootstrap_status(&state)).await;
    assert_eq!(body["bootstrap_required"], false);
}

#[tokio::test]
async fn bootstrap_status_false_when_operators_exist_even_without_sentinel() {
    let state = fresh_bootstrap_state();
    let mut collection = crate::users::UserCollection::default();
    collection
        .add(crate::users::UserRecord {
            id: crate::users::new_user_id(),
            username: "alice".into(),
            password_hash: crate::auth::hash_password("p").unwrap(),
            created_at: 1000,
            password_changed_at: 1000,
        })
        .unwrap();
    crate::users::save(&state.users_file, &collection).unwrap();
    let body = read_json(handle_admin_bootstrap_status(&state)).await;
    assert_eq!(body["bootstrap_required"], false);
}

#[tokio::test]
async fn bootstrap_succeeds_on_empty_store_and_sets_sentinel() {
    let state = fresh_bootstrap_state();
    let resp = run_bootstrap(&state, "alice", "hunter2");
    assert_eq!(resp.status(), StatusCode::OK);
    // Sentinel is set.
    assert!(crate::users::bootstrap_completed(&state.users_file).unwrap());
    // First user record is created.
    let collection = crate::users::load(&state.users_file).unwrap();
    let u = collection.find_by_username("alice").unwrap();
    assert!(crate::auth::verify_password(&u.password_hash, "hunter2"));
    // Session and CSRF cookies are emitted by the
    // `tower-sessions` / `axum-login` / `axum_csrf` layers, not by
    // the bootstrap handler itself. The integration tests cover the
    // full layered response; here we just confirm the unit-level DB
    // side-effects.
}

#[tokio::test]
async fn bootstrap_second_call_returns_bootstrap_already_completed() {
    let state = fresh_bootstrap_state();
    let _ = run_bootstrap(&state, "alice", "hunter2");
    let resp = run_bootstrap(&state, "eve", "haxx0r");
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = read_json(resp).await;
    assert_eq!(body["code"], "bootstrap_already_completed");
}

#[tokio::test]
async fn bootstrap_refuses_when_users_exist_without_sentinel() {
    // This is the "CLI recovery happened first" case: a user
    // exists, but the sentinel was never set. The online flow
    // must still refuse with users_already_exist.
    let state = fresh_bootstrap_state();
    let mut collection = crate::users::UserCollection::default();
    collection
        .add(crate::users::UserRecord {
            id: crate::users::new_user_id(),
            username: "rescuer".into(),
            password_hash: crate::auth::hash_password("rescue").unwrap(),
            created_at: 1000,
            password_changed_at: 1000,
        })
        .unwrap();
    crate::users::save(&state.users_file, &collection).unwrap();

    let resp = run_bootstrap(&state, "newbie", "x");
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = read_json(resp).await;
    assert_eq!(body["code"], "users_already_exist");
}

#[tokio::test]
async fn bootstrap_concurrent_calls_only_one_succeeds() {
    let state = Arc::new(fresh_bootstrap_state());
    let s1 = state.clone();
    let s2 = state.clone();
    let h1 = tokio::task::spawn_blocking(move || run_bootstrap(&s1, "alice", "p1"));
    let h2 = tokio::task::spawn_blocking(move || run_bootstrap(&s2, "bob", "p2"));
    let r1 = h1.await.unwrap();
    let r2 = h2.await.unwrap();
    let statuses = [r1.status(), r2.status()];
    let ok_count = statuses.iter().filter(|s| **s == StatusCode::OK).count();
    let conflict_count = statuses
        .iter()
        .filter(|s| **s == StatusCode::CONFLICT)
        .count();
    assert_eq!(ok_count, 1, "exactly one bootstrap must win: {statuses:?}");
    assert_eq!(conflict_count, 1, "the loser must see 409: {statuses:?}");
    // Sentinel is set and collection has exactly one operator.
    assert!(crate::users::bootstrap_completed(&state.users_file).unwrap());
    let collection = crate::users::load(&state.users_file).unwrap();
    assert_eq!(collection.len(), 1);
}
