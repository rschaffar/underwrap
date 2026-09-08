use std::collections::HashMap;
use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use hyper::StatusCode;
use underwrap_protocol::service::{
    AdminKeyHex, AdminStoreBackendKind, AdminStoreCreateRequest, AdminStoreMigrateRequest,
};

use super::*;
use crate::admin_api::stores::crud::handle_admin_store_create;
use crate::http::responses::Body;
use crate::store_registry::StoreState;
use crate::test_support::{
    admin_principal, block_on, crud_state, response_body_json, run_handler, tree_snapshot,
};

fn create_plain(state: Arc<ListenerState>, name: &str) -> String {
    let (status, body) = run_handler(handle_admin_store_create(
        State(state),
        Extension(admin_principal()),
        Body(AdminStoreCreateRequest {
            name: name.into(),
            backend: AdminStoreBackendKind::Plain,
            description: Some("metadata".into()),
            key_hex: None,
        }),
    ));
    assert_eq!(status, StatusCode::OK, "{body}");
    body["store"]["id"].as_str().unwrap().to_string()
}

fn migrate(
    state: Arc<ListenerState>,
    store_id: &str,
    backend: AdminStoreBackendKind,
    key_hex: Option<AdminKeyHex>,
) -> (StatusCode, serde_json::Value) {
    run_handler(handle_admin_store_migrate(
        State(state),
        Extension(admin_principal()),
        Path(store_id.to_string()),
        Body(AdminStoreMigrateRequest { backend, key_hex }),
    ))
}

#[test]
fn generated_key_response_is_exactly_non_cacheable_and_storage_neutral() {
    let tmp = tempfile::TempDir::new().unwrap();
    let _state = Arc::new(crud_state(tmp.path()));
    let before = tree_snapshot(tmp.path());
    let response = block_on(handle_admin_store_key_generate(
        Extension(admin_principal()),
    ))
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[hyper::header::CACHE_CONTROL],
        "no-store, no-cache, must-revalidate"
    );
    assert_eq!(response.headers()[hyper::header::PRAGMA], "no-cache");
    assert_eq!(response.headers()[hyper::header::EXPIRES], "0");
    let body = response_body_json(response);
    let key = body["key_hex"].as_str().unwrap();
    assert_eq!(key.len(), 64);
    assert!(
        key.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    assert_eq!(tree_snapshot(tmp.path()), before);
}

#[test]
fn submitted_creation_key_contract_is_strict_and_pre_mutation() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    for (backend, key_hex, code) in [
        (AdminStoreBackendKind::Encrypted, None, "key_required"),
        (
            AdminStoreBackendKind::Encrypted,
            Some(AdminKeyHex::new("AA".repeat(32))),
            "invalid_key_hex",
        ),
        (
            AdminStoreBackendKind::Encrypted,
            Some(AdminKeyHex::new(format!(" {}", "aa".repeat(32)))),
            "invalid_key_hex",
        ),
        (
            AdminStoreBackendKind::Plain,
            Some(AdminKeyHex::new("aa".repeat(32))),
            "unexpected_key",
        ),
    ] {
        let before = tree_snapshot(tmp.path());
        let (status, body) = run_handler(handle_admin_store_create(
            State(state.clone()),
            Extension(admin_principal()),
            Body(AdminStoreCreateRequest {
                name: "candidate".into(),
                backend,
                description: None,
                key_hex,
            }),
        ));
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], code);
        assert_eq!(tree_snapshot(tmp.path()), before);
    }

    let submitted = "ab".repeat(32);
    let (status, body) = run_handler(handle_admin_store_create(
        State(state.clone()),
        Extension(admin_principal()),
        Body(AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
            key_hex: Some(AdminKeyHex::new(submitted.clone())),
        }),
    ));
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.as_object().unwrap().contains_key("key_hex"));
    let store_id = body["store"]["id"].as_str().unwrap();
    let record = state
        .shared
        .stores
        .read()
        .unwrap()
        .entry(store_id)
        .unwrap()
        .record
        .clone();
    let mut bytes = underwrap_backend_store::parse_hex_key(&submitted).unwrap();
    let reopened = underwrap_backend_store::EncryptedBackend::open_existing_with_key(
        record.storage.dir(),
        bytes,
    )
    .unwrap();
    bytes.fill(0);
    assert_eq!(
        reopened
            .read(underwrap_backend_store::ENCRYPTED_CANARY_ID)
            .unwrap()
            .unwrap()
            .as_ref(),
        underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn handler_migrates_both_directions_with_stable_projection_and_drains_leases() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let store_id = create_plain(state.clone(), "work");
    let lease = state
        .shared
        .stores
        .read()
        .unwrap()
        .lease(&store_id)
        .unwrap();
    lease.write("nested/resource", b"logical bytes").unwrap();

    let task = tokio::spawn({
        let state = Arc::clone(&state);
        let store_id = store_id.clone();
        async move {
            handle_admin_store_migrate(
                State(state),
                Extension(admin_principal()),
                Path(store_id),
                Body(AdminStoreMigrateRequest {
                    backend: AdminStoreBackendKind::Encrypted,
                    key_hex: Some(AdminKeyHex::new("cd".repeat(32))),
                }),
            )
            .await
        }
    });
    for _ in 0..1000 {
        if matches!(
            state
                .shared
                .stores
                .read()
                .unwrap()
                .entry(&store_id)
                .unwrap()
                .state,
            StoreState::Busy
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(matches!(
        state
            .shared
            .stores
            .read()
            .unwrap()
            .entry(&store_id)
            .unwrap()
            .state,
        StoreState::Busy
    ));
    let unavailable = match crate::admin_api::stores::admin_store(&state, &store_id) {
        Ok(_) => panic!("busy store admitted a normal lease"),
        Err(error) => error.into_response(),
    };
    assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response_body_json(unavailable)["code"], "store_busy");
    drop(lease);

    let response = task.await.unwrap().into_response();
    let status = response.status();
    let body = response_body_json(response);
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["store"]["id"], store_id);
    assert_eq!(body["store"]["name"], "work");
    assert_eq!(body["store"]["description"], "metadata");
    assert_eq!(body["store"]["lifecycle_state"], "unlocked");
    assert_eq!(
        state
            .shared
            .stores
            .read()
            .unwrap()
            .expect_unlocked(&store_id)
            .read("nested/resource")
            .unwrap()
            .unwrap()
            .as_ref(),
        b"logical bytes"
    );

    let (status, body) = migrate(state.clone(), &store_id, AdminStoreBackendKind::Plain, None);
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["store"]["id"], store_id);
    assert_eq!(body["store"]["lifecycle_state"], "ready");
    let registry = state.shared.stores.read().unwrap();
    let entry = registry.entry(&store_id).unwrap();
    assert!(matches!(entry.state, StoreState::Ready(_)));
    assert_eq!(
        registry
            .expect_unlocked(&store_id)
            .list("")
            .unwrap()
            .into_iter()
            .filter(|id| id == underwrap_backend_store::ENCRYPTED_CANARY_ID)
            .count(),
        0
    );
}

#[cfg(unix)]
#[test]
fn invalid_initial_source_topology_is_recovery_required_now_and_after_restart() {
    use std::os::unix::fs::symlink;

    for topology in ["missing", "symlink", "regular-file"] {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = Arc::new(crud_state(tmp.path()));
        let store_id = create_plain(state.clone(), "work");
        let record = state
            .shared
            .stores
            .read()
            .unwrap()
            .entry(&store_id)
            .unwrap()
            .record
            .clone();
        let canonical = record.storage.dir().clone();
        std::fs::remove_dir_all(&canonical).unwrap();
        match topology {
            "missing" => {}
            "symlink" => {
                let alternate = tmp.path().join("alternate");
                std::fs::create_dir(&alternate).unwrap();
                symlink(&alternate, &canonical).unwrap();
            }
            "regular-file" => std::fs::write(&canonical, b"invalid topology").unwrap(),
            _ => unreachable!(),
        }

        let (status, body) = migrate(
            state.clone(),
            &store_id,
            AdminStoreBackendKind::Encrypted,
            Some(AdminKeyHex::new("de".repeat(32))),
        );
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{topology}");
        assert_eq!(body["code"], "migration_recovery_required", "{topology}");
        assert!(matches!(
            state
                .shared
                .stores
                .read()
                .unwrap()
                .entry(&store_id)
                .unwrap()
                .state,
            StoreState::RecoveryRequired
        ));
        assert!(
            std::fs::read_dir(canonical.parent().unwrap())
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!(".{store_id}.underwrap-migration-"))),
            "{topology}"
        );

        let restarted =
            crate::store_registry::open_initial_stores(HashMap::from([(store_id.clone(), record)]))
                .unwrap();
        assert!(matches!(
            restarted[&store_id].state,
            StoreState::RecoveryRequired
        ));
    }
}

#[test]
fn migration_direction_validation_is_fixed_and_pre_mutation() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let store_id = create_plain(state.clone(), "work");
    for (backend, key_hex, status, code) in [
        (
            AdminStoreBackendKind::Plain,
            None,
            StatusCode::CONFLICT,
            "same_backend",
        ),
        (
            AdminStoreBackendKind::Encrypted,
            None,
            StatusCode::BAD_REQUEST,
            "key_required",
        ),
        (
            AdminStoreBackendKind::Encrypted,
            Some(AdminKeyHex::new("EF".repeat(32))),
            StatusCode::BAD_REQUEST,
            "invalid_key_hex",
        ),
    ] {
        let before = tree_snapshot(tmp.path());
        let (actual_status, body) = migrate(state.clone(), &store_id, backend, key_hex);
        assert_eq!(actual_status, status);
        assert_eq!(body["code"], code);
        assert_eq!(tree_snapshot(tmp.path()), before);
    }
}
