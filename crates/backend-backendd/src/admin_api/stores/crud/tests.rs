use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, Query, State};
use hyper::StatusCode;
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::service::{
    AdminStoreBackendKind, AdminStoreCreateRequest, AdminStoreUpdateRequest,
};

use super::*;
use crate::http::responses::Body;
use crate::test_support::{
    SAMPLE_CLIENT_CERT_PEM, admin_principal, create_store_for_test, crud_state, run_handler,
};

fn create_store(
    state: Arc<crate::runtime_state::AdminRuntimeState>,
    p: &crate::users::Principal,
    body: &AdminStoreCreateRequest,
) -> (StatusCode, serde_json::Value) {
    run_handler(handle_admin_store_create(
        State(state),
        Extension(p.clone()),
        Body(body.clone()),
    ))
}

/// Tests previously asserted against the dedicated
/// `handle_admin_store_detail` endpoint. That route was removed (the
/// SPA only consumes the list), so we re-implement the same
/// projection in tests by calling `handle_admin_stores_list` and
/// finding the row by id. The list item shape is a subset of the
/// old detail shape — it omits `dir` and `source_path` — so any test
/// that needed those reads them from the registry file directly.
fn store_detail(
    state: Arc<crate::runtime_state::AdminRuntimeState>,
    store_id: &str,
) -> (StatusCode, serde_json::Value) {
    let (status, body) = run_handler(handle_admin_stores_list(State(state)));
    let item = body["items"]
        .as_array()
        .expect("items array")
        .iter()
        .find(|item| item["id"] == store_id)
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    (status, item)
}

fn store_delete(
    state: Arc<crate::runtime_state::AdminRuntimeState>,
    p: &crate::users::Principal,
    store_id: &str,
    force: bool,
) -> (StatusCode, serde_json::Value) {
    let q = DeleteQuery {
        force: force.then(|| "true".to_string()),
    };
    run_handler(handle_admin_store_delete(
        State(state),
        Extension(p.clone()),
        Path(store_id.to_string()),
        Query(q),
    ))
}

#[test]
fn list_returns_empty_when_nothing_configured() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let (status, body) = run_handler(handle_admin_stores_list(State(state)));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 0);
}

#[test]
fn admin_can_create_get_update_delete_a_dynamic_plain_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let (status, body) = create_store(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: Some("the work store".into()),
        },
    );
    assert_eq!(status, StatusCode::OK);

    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    let expected_dir = tmp.path().join("data").join(&store_id);
    assert_eq!(body["store"]["name"], "work");
    assert_eq!(body["store"]["backend"], "plain");
    assert_eq!(body["store"]["description"], "the work store");
    assert_eq!(body["store"]["usage"]["mapped_client_instance_count"], 0);
    assert_eq!(body["store"]["usage"]["has_secrets"], false);
    assert_eq!(body["store"]["actions"]["can_rotate_key"], false);
    assert_eq!(body["store"]["actions"]["can_delete_now"], true);
    assert_eq!(body["store"]["actions"]["can_force_delete"], false);
    let registry_json = std::fs::read_to_string(&state.stores_file).unwrap();
    assert!(registry_json.contains(&store_id));
    // The runtime registry sees it.
    let (_, body) = run_handler(handle_admin_stores_list(State(state.clone())));
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], store_id);
    assert_eq!(items[0]["name"], "work");
    assert_eq!(items[0]["backend"], "plain");
    assert_eq!(items[0]["description"], "the work store");
    assert_eq!(items[0]["usage"]["mapped_client_instance_count"], 0);
    assert_eq!(items[0]["usage"]["has_secrets"], false);
    assert_eq!(items[0]["actions"]["can_delete_now"], true);
    assert_eq!(items[0]["actions"]["can_force_delete"], false);
    // Detail (projection of the list row + on-disk dir).
    let (status, body) = store_detail(state.clone(), &store_id);
    assert_eq!(status, StatusCode::OK);
    assert!(expected_dir.exists(), "expected store dir to be on disk");
    assert_eq!(body["usage"]["mapped_client_instance_count"], 0);
    assert_eq!(body["usage"]["has_secrets"], false);
    assert_eq!(body["actions"]["can_delete_now"], true);
    // Update description.
    let (status, _) = run_handler(handle_admin_store_update(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
        Body(AdminStoreUpdateRequest {
            name: None,
            description: Some("renamed".into()),
        }),
    ));
    assert_eq!(status, StatusCode::OK);
    let (_, body) = store_detail(state.clone(), &store_id);
    assert_eq!(body["description"], "renamed");
    // Clearing description with an empty string nulls it out.
    let (status, _) = run_handler(handle_admin_store_update(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
        Body(AdminStoreUpdateRequest {
            name: None,
            description: Some(String::new()),
        }),
    ));
    assert_eq!(status, StatusCode::OK);
    let (_, body) = store_detail(state.clone(), &store_id);
    assert!(!body.as_object().unwrap().contains_key("description"));
    // Delete (empty store).
    let (status, body) = store_delete(state.clone(), &p, &store_id, false);
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert!(!body.as_object().unwrap().contains_key("cleanup_hint"));
    assert!(
        !state.stores_file.exists()
            || !std::fs::read_to_string(&state.stores_file)
                .unwrap_or_default()
                .contains(&store_id)
    );
}

#[test]
fn create_refuses_when_stores_data_root_unconfigured() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut owned = crud_state(tmp.path());
    owned.stores_data_root = None;
    let state = Arc::new(owned);
    let (status, body) = create_store(
        state,
        &admin_principal(),
        &AdminStoreCreateRequest {
            name: "stray".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "data_root_not_configured");
}

#[test]
fn create_refuses_duplicate_name() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let req = AdminStoreCreateRequest {
        name: "work".into(),
        backend: AdminStoreBackendKind::Plain,
        description: None,
    };
    let (first_status, _) = create_store(state.clone(), &p, &req);
    assert_eq!(first_status, StatusCode::OK);
    let (status, body) = create_store(state, &p, &req);
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "store_name_already_exists");
}

#[test]
fn delete_refuses_non_empty_without_force_and_emits_cleanup_hint_with_force() {
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
    let expected_dir = tmp.path().join("data").join(&store_id);
    // Drop a secret in so the store is non-empty.
    {
        let reg = state.stores.read().unwrap();
        reg.expect_unlocked(&store_id)
            .write("greeting", &SecretBytes::from(b"hello".to_vec()))
            .unwrap();
    }
    // Without force → 409 + code store_not_empty.
    let (status, body) = store_delete(state.clone(), &p, &store_id, false);
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "store_not_empty");
    // With force → 200 + cleanup_hint pointing at the data dir.
    let (status, body) = store_delete(state.clone(), &p, &store_id, true);
    assert_eq!(status, StatusCode::OK);
    let hint = body["cleanup_hint"]
        .as_str()
        .expect("force-delete must include a cleanup_hint when non-empty");
    assert!(hint.contains(expected_dir.to_string_lossy().as_ref()));
}

#[test]
fn create_plain_store_returns_no_key_hex() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let (status, body) = create_store(
        state,
        &p,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    assert_eq!(status, StatusCode::OK);
    assert!(
        !body.as_object().unwrap().contains_key("key_hex"),
        "plain stores must not carry a key_hex; got {body:?}",
    );
    // The store payload sits under `store`.
    assert_eq!(body["store"]["backend"], "plain");
}

#[test]
fn create_encrypted_store_generates_key_in_memory_and_returns_it_once() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let (status, body) = create_store(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
        },
    );
    assert_eq!(status, StatusCode::OK);
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    let key_hex = body["key_hex"]
        .as_str()
        .expect("encrypted create must include key_hex")
        .to_string();
    assert_eq!(key_hex.len(), 64, "key_hex must be 32 bytes hex-encoded");
    assert!(
        key_hex.chars().all(|c| c.is_ascii_hexdigit()),
        "key_hex must be lowercase hex",
    );

    // No file under the test root survives the call with a
    // `.key` suffix — the master key never touched disk. We walk
    // manually rather than pull in a new dependency.
    fn assert_no_key_files(dir: &std::path::Path) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                assert_no_key_files(&path);
            } else {
                let name = entry.file_name();
                let name_lossy = name.to_string_lossy();
                assert!(
                    !name_lossy.ends_with(".key"),
                    "no on-disk .key file should exist after create; found {}",
                    path.display(),
                );
            }
        }
    }
    assert_no_key_files(tmp.path());

    // The unlock canary lives in the data directory under its
    // reserved id; we read it back through the live registry
    // backend so we know the key in memory still decrypts it.
    let backend = state.stores.read().unwrap().expect_unlocked(&store_id);
    let canary = backend
        .read(underwrap_store::ENCRYPTED_CANARY_ID)
        .unwrap()
        .expect("canary must exist after create");
    assert_eq!(canary.as_ref(), underwrap_store::ENCRYPTED_CANARY_PLAINTEXT);

    // The detail response carries the data dir and `locked=false`
    // but never echoes the key value.
    let (_, body) = store_detail(state, &store_id);
    assert_eq!(body["backend"], "encrypted");
    assert_eq!(body["locked"], false);
    let body_str = serde_json::to_string(&body).unwrap();
    assert!(
        !body_str.contains(&key_hex),
        "key material leaked into detail response: {body_str}",
    );
}

#[test]
fn canary_is_hidden_from_admin_has_secrets_view() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let (_, body) = create_store(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
        },
    );
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    // Fresh encrypted store has only the canary; admin view must
    // not flag it as having secrets.
    let (_, detail) = store_detail(state, &store_id);
    assert_eq!(detail["usage"]["has_secrets"], false);
}

#[test]
fn store_management_view_reflects_usage_for_authenticated_user() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let admin = admin_principal();
    let plain_id = create_store_for_test(
        state.clone(),
        &admin,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    let encrypted_id = create_store_for_test(
        state.clone(),
        &admin,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
        },
    );
    {
        let reg = state.stores.read().unwrap();
        reg.expect_unlocked(&plain_id)
            .write("secret/item", b"value")
            .unwrap();
        reg.expect_unlocked(&encrypted_id)
            .write("secret/item", b"value")
            .unwrap();
    }
    state.client_instances.write().unwrap().insert(
        "client_a".into(),
        crate::runtime_clients::ClientInstanceRecord {
            id: "client_a".into(),
            display_name: "Laptop".into(),
            current_store_id: plain_id.clone(),
            status: crate::runtime_clients::ClientInstanceStatus::Active,
            last_seen_at: None,
            current_credential: crate::runtime_clients::CurrentCredentialRecord {
                cert_fingerprint: "fp-1".into(),
                cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
            },
            auto_create_wrapper_assets: false,
        },
    );

    let (_, list) = run_handler(handle_admin_stores_list(State(state.clone())));
    let items = list["items"].as_array().unwrap();
    let plain = items.iter().find(|item| item["id"] == plain_id).unwrap();
    assert_eq!(plain["usage"]["mapped_client_instance_count"], 1);
    assert_eq!(plain["usage"]["has_secrets"], true);
    assert_eq!(plain["actions"]["can_rotate_key"], false);
    assert_eq!(plain["actions"]["can_delete_now"], false);
    assert_eq!(plain["actions"]["can_force_delete"], false);

    let encrypted = items
        .iter()
        .find(|item| item["id"] == encrypted_id)
        .unwrap();
    assert_eq!(encrypted["usage"]["mapped_client_instance_count"], 0);
    assert_eq!(encrypted["usage"]["has_secrets"], true);
    assert_eq!(encrypted["actions"]["can_rotate_key"], true);
    assert_eq!(encrypted["actions"]["can_delete_now"], false);
    assert_eq!(encrypted["actions"]["can_force_delete"], true);

    let (_, detail) = store_detail(state, &plain_id);
    assert_eq!(detail["usage"]["mapped_client_instance_count"], 1);
    assert_eq!(detail["usage"]["has_secrets"], true);
    assert_eq!(detail["actions"]["can_rotate_key"], false);
    assert_eq!(detail["actions"]["can_delete_now"], false);
    assert_eq!(detail["actions"]["can_force_delete"], false);
}
