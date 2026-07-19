use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, State};
use hyper::StatusCode;
use underwrap_protocol::service::{AdminStoreBackendKind, AdminStoreCreateRequest};

use super::*;
use crate::admin_api::stores::crud::{handle_admin_store_create, handle_admin_stores_list};
use crate::http::responses::Body;
use crate::runtime_state::ListenerState;
use crate::store_registry::StoreState;
use crate::test_support::{admin_principal, create_store_for_test, crud_state, run_handler};

fn is_locked(state: &ListenerState, store_id: &str) -> bool {
    matches!(
        state.shared.stores.read().unwrap().entry(store_id),
        Ok(entry) if matches!(entry.state, StoreState::Locked),
    )
}

fn create_store(
    state: Arc<crate::runtime_state::ListenerState>,
    p: &crate::admin_api::csrf::Principal,
    backend: AdminStoreBackendKind,
    name: &str,
) -> serde_json::Value {
    let (status, body) = run_handler(handle_admin_store_create(
        State(state),
        Extension(p.clone()),
        Body(AdminStoreCreateRequest {
            name: name.into(),
            backend,
            description: None,
        }),
    ));
    assert_eq!(status, StatusCode::OK, "store create failed: {body}");
    body
}

#[tokio::test(flavor = "multi_thread")]
async fn rotate_key_reencrypts_resources_and_returns_new_value_once() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let create_body = create_store(state.clone(), &p, AdminStoreBackendKind::Encrypted, "vault");
    let store_id = create_body["store"]["id"].as_str().unwrap().to_string();
    let original_key = create_body["key_hex"].as_str().unwrap().to_string();

    // Seed a known plaintext under the original key.
    {
        let backend = state
            .shared
            .stores
            .read()
            .unwrap()
            .expect_unlocked(&store_id);
        backend.write("greeting", b"hello").unwrap();
    }

    let (status, body) = run_handler(handle_admin_store_rotate_key(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
    ));
    assert_eq!(status, StatusCode::OK);
    let new_key = body["key_hex"].as_str().unwrap().to_string();
    assert_ne!(new_key, original_key, "rotation must produce a fresh key");
    assert_eq!(new_key.len(), 64);
    assert!(
        body["warning"]
            .as_str()
            .unwrap()
            .contains("re-encrypted under the new master key"),
        "warning must describe the re-encryption: {}",
        body["warning"],
    );

    // The active backend still decodes the same plaintext (now
    // under the new key).
    {
        let backend = state
            .shared
            .stores
            .read()
            .unwrap()
            .expect_unlocked(&store_id);
        assert_eq!(
            backend.read("greeting").unwrap().unwrap().as_ref(),
            b"hello"
        );
        // Canary survived the rotation.
        assert_eq!(
            backend
                .read(underwrap_backend_store::ENCRYPTED_CANARY_ID)
                .unwrap()
                .unwrap()
                .as_ref(),
            underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT,
        );
    }

    // Store-list payload still does not leak the key value.
    let (_, body) = run_handler(handle_admin_stores_list(State(state.clone())));
    let body_str = serde_json::to_string(&body).unwrap();
    assert!(!body_str.contains(&new_key));
    assert!(!body_str.contains(&original_key));
}

#[tokio::test(flavor = "multi_thread")]
async fn rotate_key_refuses_plain_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let store_id = create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    let (status, body) = run_handler(handle_admin_store_rotate_key(
        State(state.clone()),
        Extension(p),
        Path(store_id),
    ));
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "not_encrypted");
}

#[tokio::test(flavor = "multi_thread")]
async fn rotate_key_refuses_locked_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let store_id = create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
        },
    );
    // Lock the store and confirm rotation refuses.
    let (lock_status, _) = run_handler(handle_admin_store_lock(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
    ));
    assert_eq!(lock_status, StatusCode::OK);
    let (status, body) = run_handler(handle_admin_store_rotate_key(
        State(state.clone()),
        Extension(p),
        Path(store_id),
    ));
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "store_locked");
}

#[test]
fn unlock_round_trip_succeeds_with_correct_hex() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let body = create_store(state.clone(), &p, AdminStoreBackendKind::Encrypted, "vault");
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    let key_hex = body["key_hex"].as_str().unwrap().to_string();

    // Simulate a restart by locking the store.
    let (lock_status, _) = run_handler(handle_admin_store_lock(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
    ));
    assert_eq!(lock_status, StatusCode::OK);
    assert!(is_locked(&state, &store_id));

    let (status, _) = run_handler(handle_admin_store_unlock(
        State(state.clone()),
        Extension(p),
        Path(store_id.clone()),
        Body(underwrap_protocol::service::AdminStoreUnlockRequest { key_hex }),
    ));
    assert_eq!(status, StatusCode::OK);
    assert!(!is_locked(&state, &store_id));
}

#[test]
fn unlock_rejects_wrong_hex_and_keeps_store_locked() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let body = create_store(state.clone(), &p, AdminStoreBackendKind::Encrypted, "vault");
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    let (lock_status, _) = run_handler(handle_admin_store_lock(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
    ));
    assert_eq!(lock_status, StatusCode::OK);

    let bogus_hex = "0".repeat(64);
    let (status, resp_body) = run_handler(handle_admin_store_unlock(
        State(state.clone()),
        Extension(p),
        Path(store_id.clone()),
        Body(underwrap_protocol::service::AdminStoreUnlockRequest { key_hex: bogus_hex }),
    ));
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(resp_body["code"], "key_mismatch");
    assert!(is_locked(&state, &store_id));
}

#[test]
fn unlock_rejects_invalid_hex_format() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let body = create_store(state.clone(), &p, AdminStoreBackendKind::Encrypted, "vault");
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    let (lock_status, _) = run_handler(handle_admin_store_lock(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
    ));
    assert_eq!(lock_status, StatusCode::OK);
    let (status, resp_body) = run_handler(handle_admin_store_unlock(
        State(state.clone()),
        Extension(p),
        Path(store_id),
        Body(underwrap_protocol::service::AdminStoreUnlockRequest {
            key_hex: "not hex".into(),
        }),
    ));
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(resp_body["code"], "invalid_key_hex");
}

#[test]
fn unlock_refuses_plain_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let store_id = create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    let (status, body) = run_handler(handle_admin_store_unlock(
        State(state.clone()),
        Extension(p),
        Path(store_id),
        Body(underwrap_protocol::service::AdminStoreUnlockRequest {
            key_hex: "0".repeat(64),
        }),
    ));
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "store_not_encrypted");
}

#[test]
fn unlock_refuses_already_unlocked_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let body = create_store(state.clone(), &p, AdminStoreBackendKind::Encrypted, "vault");
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    let key_hex = body["key_hex"].as_str().unwrap().to_string();
    let (status, resp_body) = run_handler(handle_admin_store_unlock(
        State(state.clone()),
        Extension(p),
        Path(store_id),
        Body(underwrap_protocol::service::AdminStoreUnlockRequest { key_hex }),
    ));
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(resp_body["code"], "already_unlocked");
}

#[test]
fn lock_transitions_unlocked_encrypted_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let body = create_store(state.clone(), &p, AdminStoreBackendKind::Encrypted, "vault");
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    assert!(!is_locked(&state, &store_id));
    let (status, body) = run_handler(handle_admin_store_lock(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["was_locked"], false);
    assert!(is_locked(&state, &store_id));
    // Second call is a no-op success.
    let (status, body) = run_handler(handle_admin_store_lock(
        State(state.clone()),
        Extension(p),
        Path(store_id),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["was_locked"], true);
}

#[test]
fn lock_refuses_plain_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let store_id = create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    let (status, body) = run_handler(handle_admin_store_lock(
        State(state.clone()),
        Extension(p),
        Path(store_id),
    ));
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "store_not_encrypted");
}
