use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::symlink;
use std::sync::Arc;

use axum::extract::State;
use hyper::StatusCode;

use super::*;
use crate::test_support::{admin_mut, fresh_bootstrap_state, run_handler};

fn status(state: Arc<ListenerState>) -> (StatusCode, serde_json::Value) {
    run_handler(handle_admin_bootstrap_status(State(state)))
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_status_required_on_fresh_store() {
    let (response_status, body) = status(Arc::new(fresh_bootstrap_state()));
    assert_eq!(response_status, StatusCode::OK);
    assert_eq!(body["bootstrap_required"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_status_false_when_sentinel_set() {
    let state = Arc::new(fresh_bootstrap_state());
    crate::users::mark_bootstrap_completed(&state.admin().bootstrap_file).unwrap();
    assert_eq!(status(state).1["bootstrap_required"], false);
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
    crate::users::save_user(&state.admin().users_dir, &record).unwrap();
    assert_eq!(status(state).1["bootstrap_required"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_status_fails_closed_on_malformed_operator_state() {
    let state = Arc::new(fresh_bootstrap_state());
    std::fs::create_dir_all(&state.admin().users_dir).unwrap();
    std::fs::write(state.admin().users_dir.join("broken.json"), b"not-json").unwrap();
    assert_eq!(status(state).1["bootstrap_required"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_status_fails_closed_on_dangling_sentinel_symlink() {
    let state = Arc::new(fresh_bootstrap_state());
    std::fs::create_dir_all(state.admin().bootstrap_file.parent().unwrap()).unwrap();
    symlink("missing-sentinel-target", &state.admin().bootstrap_file).unwrap();

    assert_eq!(status(state).1["bootstrap_required"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_status_fails_closed_when_sentinel_cannot_be_read() {
    let mut state = fresh_bootstrap_state();
    admin_mut(&mut state).bootstrap_file = OsString::from_vec(b"invalid\0path".to_vec()).into();
    assert_eq!(status(Arc::new(state)).1["bootstrap_required"], false);
}
