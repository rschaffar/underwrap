use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, State};
use hyper::StatusCode;
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::service::{
    AdminStoreBackendKind, AdminStoreCreateRequest, AdminStoreUpdateRequest,
};

use super::*;
use crate::http::responses::Body;
use crate::test_support::{
    BlockedRegistryDir, SAMPLE_CLIENT_CERT_PEM, admin_principal, capture_tracing_output,
    create_store_for_test, crud_state, delete_store_for_test, run_handler,
};

fn assert_registry_failure(
    body: &serde_json::Value,
    log: &str,
    registry_dir: &std::path::Path,
    context: &str,
) {
    assert_eq!(
        body,
        &serde_json::json!({
            "code": "write_store_registry_failed",
            "message": "could not persist store registry",
        }),
    );
    let registry_path = registry_dir.to_string_lossy();
    assert!(!body.to_string().contains(registry_path.as_ref()));
    assert!(log.contains("ERROR admin internal failure"), "{log}");
    assert!(
        log.contains("response_code=\"write_store_registry_failed\""),
        "{log}",
    );
    assert!(log.contains(&format!("context=\"{context}\"")), "{log}");
    assert!(log.contains(registry_path.as_ref()), "{log}");
}

fn create_store(
    state: Arc<crate::runtime_state::ListenerState>,
    p: &crate::admin_api::csrf::Principal,
    body: &AdminStoreCreateRequest,
) -> (StatusCode, serde_json::Value) {
    run_handler(handle_admin_store_create(
        State(state),
        Extension(p.clone()),
        Body(body.clone()),
    ))
}

/// Return one store's list projection for handler tests. The admin surface has
/// no detail route because the SPA consumes the list; fields outside the list
/// item shape are asserted directly against the persisted registry record.
fn store_detail(
    state: Arc<crate::runtime_state::ListenerState>,
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
            key_hex: None,
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
    // The per-store record landed on disk under the configured stores
    // directory.
    let record_path = state.admin().stores_dir.join(format!("{store_id}.json"));
    assert!(record_path.exists(), "store record file must be created");
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
    let (status, body) = delete_store_for_test(state.clone(), &p, &store_id, false);
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert!(!body.as_object().unwrap().contains_key("cleanup_hint"));
    assert!(
        !state
            .admin()
            .stores_dir
            .join(format!("{store_id}.json"))
            .exists(),
        "store record file must be removed by delete"
    );
}

#[test]
fn create_refuses_when_stores_data_root_unconfigured() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut owned = crud_state(tmp.path());
    crate::test_support::admin_mut(&mut owned).stores_data_root = None;
    let state = Arc::new(owned);
    let (status, body) = create_store(
        state,
        &admin_principal(),
        &AdminStoreCreateRequest {
            name: "stray".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
            key_hex: None,
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
        key_hex: None,
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
            key_hex: None,
        },
    );
    let expected_dir = tmp.path().join("data").join(&store_id);
    // Drop a secret in so the store is non-empty.
    {
        let reg = state.shared.stores.read().unwrap();
        reg.expect_unlocked(&store_id)
            .write("greeting", &SecretBytes::from(b"hello".to_vec()))
            .unwrap();
    }
    // Without force → 409 + code store_not_empty.
    let (status, body) = delete_store_for_test(state.clone(), &p, &store_id, false);
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "store_not_empty");
    // With force → 200 + cleanup_hint pointing at the data dir.
    let (status, body) = delete_store_for_test(state.clone(), &p, &store_id, true);
    assert_eq!(status, StatusCode::OK);
    let hint = body["cleanup_hint"]
        .as_object()
        .expect("force-delete must include a structured cleanup_hint when non-empty");
    let data_dir = hint["data_dir"].as_str().unwrap();
    let command = hint["command"].as_str().unwrap();
    let expected = expected_dir.to_string_lossy();
    assert_eq!(data_dir, expected.as_ref());
    assert!(command.contains(expected.as_ref()));
    assert!(command.starts_with("rm -rf "));
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
            key_hex: None,
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
fn create_encrypted_store_uses_submitted_key_and_returns_no_key() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let data_root = state.admin().stores_data_root.clone().unwrap();
    std::fs::remove_dir(&data_root).unwrap();
    let p = admin_principal();
    let key_hex = "11".repeat(32);
    let (status, body) = create_store(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
            key_hex: Some(underwrap_protocol::service::AdminKeyHex::new(
                key_hex.clone(),
            )),
        },
    );
    assert_eq!(status, StatusCode::OK);
    assert!(!body.as_object().unwrap().contains_key("key_hex"));
    let store_id = body["store"]["id"].as_str().unwrap().to_string();

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
    let backend = state
        .shared
        .stores
        .read()
        .unwrap()
        .expect_unlocked(&store_id);
    let canary = backend
        .read(underwrap_backend_store::ENCRYPTED_CANARY_ID)
        .unwrap()
        .expect("canary must exist after create");
    assert_eq!(
        canary.as_ref(),
        underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT
    );

    // The detail response projects the explicit lifecycle state and never
    // echoes the submitted key value.
    let (_, body) = store_detail(state, &store_id);
    assert_eq!(body["backend"], "encrypted");
    assert_eq!(body["lifecycle_state"], "unlocked");
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
            key_hex: Some(underwrap_protocol::service::AdminKeyHex::new(
                "11".repeat(32),
            )),
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
            key_hex: None,
        },
    );
    let encrypted_id = create_store_for_test(
        state.clone(),
        &admin,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
            key_hex: Some(underwrap_protocol::service::AdminKeyHex::new(
                "11".repeat(32),
            )),
        },
    );
    {
        let reg = state.shared.stores.read().unwrap();
        reg.expect_unlocked(&plain_id)
            .write("secret/item", b"value")
            .unwrap();
        reg.expect_unlocked(&encrypted_id)
            .write("secret/item", b"value")
            .unwrap();
    }
    state.shared.client_instances.write().unwrap().insert(
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

#[test]
fn f015_store_create_blocked_registry_publishes_no_candidate_and_retries_cleanly() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let request = AdminStoreCreateRequest {
        name: "candidate".into(),
        backend: AdminStoreBackendKind::Plain,
        description: Some("candidate-description".into()),
        key_hex: None,
    };

    let ((status, body), audit) = capture_tracing_output(|| {
        let _blocked = BlockedRegistryDir::new(&state.admin().stores_dir);
        create_store(state.clone(), &principal, &request)
    });
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_registry_failure(
        &body,
        &audit,
        &state.admin().stores_dir,
        "persisting created store registry record",
    );
    assert!(
        !audit.contains("action=store.create"),
        "failed create emitted a success audit: {audit}",
    );
    assert!(
        state.shared.stores.read().unwrap().entries.is_empty(),
        "failed create published a live store candidate",
    );
    let reloaded = crate::store_registry::load_store_records(&state.admin().stores_dir).unwrap();
    assert!(
        reloaded.is_empty(),
        "failed create left a restored-reload store candidate: {reloaded:?}",
    );

    let (retry_status, retry_body) = create_store(state.clone(), &principal, &request);
    assert_eq!(retry_status, StatusCode::OK, "{retry_body}");
    let store_id = retry_body["store"]["id"].as_str().unwrap();
    let live = state
        .shared
        .stores
        .read()
        .unwrap()
        .entries
        .get(store_id)
        .unwrap()
        .record
        .clone();
    let reloaded = crate::store_registry::load_store_records(&state.admin().stores_dir).unwrap();
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded.get(store_id), Some(&live));
}

#[test]
fn f015_store_update_persists_before_publishing_live_record() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let store_id = create_store_for_test(
        state.clone(),
        &principal,
        &AdminStoreCreateRequest {
            name: "before".into(),
            backend: AdminStoreBackendKind::Plain,
            description: Some("before-description".into()),
            key_hex: None,
        },
    );
    let (before, entry_address, backend, lifecycle_lock) = {
        let stores = state.shared.stores.read().unwrap();
        let entry = stores.entries.get(&store_id).unwrap();
        (
            entry.record.clone(),
            std::ptr::from_ref(entry),
            entry.state.backend().unwrap().clone(),
            entry.lifecycle_lock.clone(),
        )
    };
    let mut candidate = before.clone();
    candidate.name = "after".into();
    candidate.description = Some("after-description".into());
    let request = AdminStoreUpdateRequest {
        name: Some(candidate.name.clone()),
        description: candidate.description.clone(),
    };

    let ((status, body), audit) = capture_tracing_output(|| {
        let _blocked = BlockedRegistryDir::new(&state.admin().stores_dir);
        run_handler(handle_admin_store_update(
            State(state.clone()),
            Extension(principal.clone()),
            Path(store_id.clone()),
            Body(request.clone()),
        ))
    });
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_registry_failure(
        &body,
        &audit,
        &state.admin().stores_dir,
        "persisting updated store registry record",
    );
    assert!(
        !audit.contains("action=store.update"),
        "failed update emitted a success audit: {audit}",
    );
    {
        let stores = state.shared.stores.read().unwrap();
        let live = stores.entries.get(&store_id).unwrap();
        assert_eq!(live.record, before);
        assert_eq!(std::ptr::from_ref(live), entry_address);
        assert!(
            std::ptr::eq(live.state.backend().unwrap().as_ref(), backend.as_ref()),
            "failed update replaced the live backend/state",
        );
        assert!(Arc::ptr_eq(&live.lifecycle_lock, &lifecycle_lock));
    }
    let reloaded = crate::store_registry::load_store_records(&state.admin().stores_dir).unwrap();
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded.get(&store_id), Some(&before));

    let (retry_status, retry_body) = run_handler(handle_admin_store_update(
        State(state.clone()),
        Extension(principal),
        Path(store_id.clone()),
        Body(request),
    ));
    assert_eq!(retry_status, StatusCode::OK, "{retry_body}");
    {
        let stores = state.shared.stores.read().unwrap();
        let live = stores.entries.get(&store_id).unwrap();
        assert_eq!(live.record, candidate);
        assert_eq!(std::ptr::from_ref(live), entry_address);
        assert!(std::ptr::eq(
            live.state.backend().unwrap().as_ref(),
            backend.as_ref(),
        ));
        assert!(Arc::ptr_eq(&live.lifecycle_lock, &lifecycle_lock));
    }
    let reloaded = crate::store_registry::load_store_records(&state.admin().stores_dir).unwrap();
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded.get(&store_id), Some(&candidate));
}

#[test]
fn f015_store_delete_persists_before_removing_live_entry() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let store_id = create_store_for_test(
        state.clone(),
        &principal,
        &AdminStoreCreateRequest {
            name: "keep".into(),
            backend: AdminStoreBackendKind::Plain,
            description: Some("keep-description".into()),
            key_hex: None,
        },
    );
    let record_path = state.admin().stores_dir.join(format!("{store_id}.json"));
    let (before, entry_address, backend, lifecycle_lock) = {
        let stores = state.shared.stores.read().unwrap();
        let entry = stores.entries.get(&store_id).unwrap();
        (
            entry.record.clone(),
            std::ptr::from_ref(entry),
            entry.state.backend().unwrap().clone(),
            entry.lifecycle_lock.clone(),
        )
    };

    let ((status, body), audit) = capture_tracing_output(|| {
        let _blocked = BlockedRegistryDir::new(&state.admin().stores_dir);
        delete_store_for_test(state.clone(), &principal, &store_id, false)
    });
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_registry_failure(
        &body,
        &audit,
        &state.admin().stores_dir,
        "deleting store registry record",
    );
    assert!(
        !audit.contains("action=store.delete"),
        "failed delete emitted a success audit: {audit}",
    );
    {
        let stores = state.shared.stores.read().unwrap();
        let live = stores.entries.get(&store_id).unwrap();
        assert_eq!(live.record, before);
        assert_eq!(std::ptr::from_ref(live), entry_address);
        assert!(
            std::ptr::eq(live.state.backend().unwrap().as_ref(), backend.as_ref()),
            "failed delete replaced the live backend/state",
        );
        assert!(Arc::ptr_eq(&live.lifecycle_lock, &lifecycle_lock));
    }
    assert!(record_path.exists());
    let reloaded = crate::store_registry::load_store_records(&state.admin().stores_dir).unwrap();
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded.get(&store_id), Some(&before));

    let (retry_status, retry_body) =
        delete_store_for_test(state.clone(), &principal, &store_id, false);
    assert_eq!(retry_status, StatusCode::OK, "{retry_body}");
    assert!(
        !state
            .shared
            .stores
            .read()
            .unwrap()
            .entries
            .contains_key(&store_id)
    );
    assert!(!record_path.exists());
    assert!(
        !crate::store_registry::load_store_records(&state.admin().stores_dir)
            .unwrap()
            .contains_key(&store_id),
    );
}
