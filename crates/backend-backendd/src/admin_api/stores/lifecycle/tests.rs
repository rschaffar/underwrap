use super::*;
use crate::admin_api::stores::crud::{handle_admin_store_create, handle_admin_store_detail};
use crate::test_support::{admin_principal, create_store_for_test, crud_state, response_body_json};
use hyper::StatusCode;
use underwrap_protocol::service::{AdminStoreBackendKind, AdminStoreCreateRequest};

#[tokio::test(flavor = "multi_thread")]
async fn rotate_key_reencrypts_resources_and_returns_new_value_once() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let p = admin_principal(&[]);
    let resp = handle_admin_store_create(
        &state,
        &p,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
        },
    );
    assert_eq!(resp.status(), StatusCode::OK);
    let create_body = response_body_json(resp);
    let store_id = create_body["store"]["id"].as_str().unwrap().to_string();
    let original_key = create_body["key_hex"].as_str().unwrap().to_string();

    // Seed a known plaintext under the original key.
    {
        let backend = state.stores.read().unwrap().expect_unlocked(&store_id);
        backend.write("greeting", b"hello").unwrap();
    }

    let resp = handle_admin_store_rotate_key(&state, &p, &store_id).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = response_body_json(resp);
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
        let backend = state.stores.read().unwrap().expect_unlocked(&store_id);
        assert_eq!(
            backend.read("greeting").unwrap().unwrap().as_ref(),
            b"hello"
        );
        // Canary survived the rotation.
        assert_eq!(
            backend
                .read(underwrap_store::ENCRYPTED_CANARY_ID)
                .unwrap()
                .unwrap()
                .as_ref(),
            underwrap_store::ENCRYPTED_CANARY_PLAINTEXT,
        );
    }

    // Detail GET still does not leak the key value.
    let resp = handle_admin_store_detail(&state, &store_id);
    let body = response_body_json(resp);
    let body_str = serde_json::to_string(&body).unwrap();
    assert!(!body_str.contains(&new_key));
    assert!(!body_str.contains(&original_key));
}

#[tokio::test(flavor = "multi_thread")]
async fn rotate_key_refuses_plain_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let p = admin_principal(&[]);
    let store_id = create_store_for_test(
        &state,
        &p,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    let resp = handle_admin_store_rotate_key(&state, &p, &store_id).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    assert_eq!(response_body_json(resp)["code"], "not_encrypted");
}

#[tokio::test(flavor = "multi_thread")]
async fn rotate_key_refuses_locked_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let p = admin_principal(&[]);
    let store_id = create_store_for_test(
        &state,
        &p,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
        },
    );
    // Lock the store and confirm rotation refuses.
    let resp = handle_admin_store_lock(&state, &p, &store_id);
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = handle_admin_store_rotate_key(&state, &p, &store_id).await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response_body_json(resp)["code"], "store_locked");
}

#[tokio::test(flavor = "multi_thread")]
async fn rotate_key_succeeds_on_unlocked_encrypted_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let p = admin_principal(&[]);
    let store_id = create_store_for_test(
        &state,
        &p,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
        },
    );
    let resp = handle_admin_store_rotate_key(&state, &p, &store_id).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[test]
fn unlock_round_trip_succeeds_with_correct_hex() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let p = admin_principal(&[]);
    let resp = handle_admin_store_create(
        &state,
        &p,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
        },
    );
    let body = response_body_json(resp);
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    let key_hex = body["key_hex"].as_str().unwrap().to_string();

    // Simulate a restart by locking the store.
    let resp = handle_admin_store_lock(&state, &p, &store_id);
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(state.stores.read().unwrap().is_locked(&store_id));

    let resp = handle_admin_store_unlock(
        &state,
        &p,
        &store_id,
        &underwrap_protocol::service::AdminStoreUnlockRequest { key_hex },
    );
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(!state.stores.read().unwrap().is_locked(&store_id));
}

#[test]
fn unlock_rejects_wrong_hex_and_keeps_store_locked() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let p = admin_principal(&[]);
    let resp = handle_admin_store_create(
        &state,
        &p,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
        },
    );
    let store_id = response_body_json(resp)["store"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let resp = handle_admin_store_lock(&state, &p, &store_id);
    assert_eq!(resp.status(), StatusCode::OK);

    let bogus_hex = "0".repeat(64);
    let resp = handle_admin_store_unlock(
        &state,
        &p,
        &store_id,
        &underwrap_protocol::service::AdminStoreUnlockRequest { key_hex: bogus_hex },
    );
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(response_body_json(resp)["code"], "key_mismatch");
    assert!(state.stores.read().unwrap().is_locked(&store_id));
}

#[test]
fn unlock_rejects_invalid_hex_format() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let p = admin_principal(&[]);
    let resp = handle_admin_store_create(
        &state,
        &p,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
        },
    );
    let store_id = response_body_json(resp)["store"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let resp = handle_admin_store_lock(&state, &p, &store_id);
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = handle_admin_store_unlock(
        &state,
        &p,
        &store_id,
        &underwrap_protocol::service::AdminStoreUnlockRequest {
            key_hex: "not hex".into(),
        },
    );
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response_body_json(resp)["code"], "invalid_key_hex");
}

#[test]
fn unlock_refuses_plain_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let p = admin_principal(&[]);
    let store_id = create_store_for_test(
        &state,
        &p,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    let resp = handle_admin_store_unlock(
        &state,
        &p,
        &store_id,
        &underwrap_protocol::service::AdminStoreUnlockRequest {
            key_hex: "0".repeat(64),
        },
    );
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response_body_json(resp)["code"], "store_not_encrypted");
}

#[test]
fn unlock_refuses_already_unlocked_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let p = admin_principal(&[]);
    let resp = handle_admin_store_create(
        &state,
        &p,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
        },
    );
    let body = response_body_json(resp);
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    let key_hex = body["key_hex"].as_str().unwrap().to_string();
    let resp = handle_admin_store_unlock(
        &state,
        &p,
        &store_id,
        &underwrap_protocol::service::AdminStoreUnlockRequest { key_hex },
    );
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    assert_eq!(response_body_json(resp)["code"], "already_unlocked");
}

#[test]
fn lock_transitions_unlocked_encrypted_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let p = admin_principal(&[]);
    let resp = handle_admin_store_create(
        &state,
        &p,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
        },
    );
    let store_id = response_body_json(resp)["store"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!state.stores.read().unwrap().is_locked(&store_id));
    let resp = handle_admin_store_lock(&state, &p, &store_id);
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(response_body_json(resp)["was_locked"], false);
    assert!(state.stores.read().unwrap().is_locked(&store_id));
    // Second call is a no-op success.
    let resp = handle_admin_store_lock(&state, &p, &store_id);
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(response_body_json(resp)["was_locked"], true);
}

#[test]
fn lock_refuses_plain_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let p = admin_principal(&[]);
    let store_id = create_store_for_test(
        &state,
        &p,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    let resp = handle_admin_store_lock(&state, &p, &store_id);
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response_body_json(resp)["code"], "store_not_encrypted");
}
