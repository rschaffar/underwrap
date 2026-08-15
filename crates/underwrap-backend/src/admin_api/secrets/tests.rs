use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, Query, State};
use base64::Engine;
use hyper::StatusCode;
use underwrap_backend_store::{StorageBackend, load_resource_blob, save_resource_blob};
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource_blob::ResourceBlob;

use super::*;
use crate::http::responses::ResourceBody;
use crate::runtime_state::ListenerState;
use crate::test_support::{
    TestStore, admin_principal, create_resource_id_for_test, entity_test_state_with_store,
    mixed_resource_blob_for_test, run_handler, test_admin_state,
};

fn principal() -> crate::admin_api::csrf::Principal {
    crate::admin_api::csrf::Principal {
        id: "alice-id".into(),
        username: "alice".into(),
    }
}

fn unlocked_store(state: &ListenerState, store_id: &str) -> Arc<dyn StorageBackend> {
    state
        .shared
        .stores
        .read()
        .unwrap()
        .expect_unlocked(store_id)
}

fn mixed_state() -> (
    tempfile::TempDir,
    Arc<ListenerState>,
    String,
    String,
    ResourceBlob,
) {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let resource_id =
        create_resource_id_for_test(state.clone(), &admin_principal(), &store_id, "github/token");
    let blob = mixed_resource_blob_for_test();
    save_resource_blob(
        unlocked_store(&state, &store_id).as_ref(),
        &resource_id,
        &blob,
    )
    .unwrap();
    (tmp, state, store_id, resource_id, blob)
}

fn assert_resource(
    state: &ListenerState,
    store_id: &str,
    resource_id: &str,
    expected: &ResourceBlob,
) {
    assert_eq!(
        load_resource_blob(unlocked_store(state, store_id).as_ref(), resource_id).unwrap(),
        *expected,
    );
}

fn rename(
    state: Arc<ListenerState>,
    store_id: String,
    resource_id: String,
    slot: &str,
    from_key: &str,
    to_key: &str,
    material: Option<&[u8]>,
) -> (StatusCode, serde_json::Value) {
    run_handler(handle_admin_secret_rename(
        State(state),
        Extension(principal()),
        Path((
            store_id,
            resource_id,
            slot.to_string(),
            from_key.to_string(),
        )),
        ResourceBody(underwrap_protocol::service::AdminSecretRenameRequest {
            to_key: to_key.to_string(),
            material_b64: material
                .map(|value| base64::engine::general_purpose::STANDARD.encode(value)),
        }),
    ))
}

#[test]
fn admin_secret_rename_preserves_mixed_resource_sections() {
    let (_tmp, state, store_id, resource_id, mut expected) = mixed_state();

    let (status, _) = rename(
        state.clone(),
        store_id.clone(),
        resource_id.clone(),
        "files",
        "config/primary.toml",
        "config/renamed.toml",
        None,
    );
    assert_eq!(status, StatusCode::OK);
    let material = expected.files.remove("config/primary.toml").unwrap();
    expected
        .files
        .insert("config/renamed.toml".into(), material);
    assert_resource(&state, &store_id, &resource_id, &expected);

    let (status, _) = rename(
        state.clone(),
        store_id.clone(),
        resource_id.clone(),
        "env",
        "PRIMARY_TOKEN",
        "RENAMED_TOKEN",
        Some(b"replacement-env"),
    );
    assert_eq!(status, StatusCode::OK);
    expected.env.remove("PRIMARY_TOKEN");
    expected
        .env
        .insert("RENAMED_TOKEN".into(), SecretBytes::from("replacement-env"));
    assert_resource(&state, &store_id, &resource_id, &expected);
}

#[test]
fn admin_secret_same_key_rename_preserves_or_replaces_material() {
    let (_tmp, state, store_id, resource_id, mut expected) = mixed_state();

    let (status, _) = rename(
        state.clone(),
        store_id.clone(),
        resource_id.clone(),
        "env",
        "PRIMARY_TOKEN",
        "PRIMARY_TOKEN",
        None,
    );
    assert_eq!(status, StatusCode::OK);
    assert_resource(&state, &store_id, &resource_id, &expected);

    let (status, _) = rename(
        state.clone(),
        store_id.clone(),
        resource_id.clone(),
        "env",
        "PRIMARY_TOKEN",
        "PRIMARY_TOKEN",
        Some(b"primary-v2"),
    );
    assert_eq!(status, StatusCode::OK);
    expected
        .env
        .insert("PRIMARY_TOKEN".into(), SecretBytes::from("primary-v2"));
    assert_resource(&state, &store_id, &resource_id, &expected);
}

#[test]
fn admin_secret_rename_missing_source_leaves_resource_unchanged() {
    let (_tmp, state, store_id, resource_id, expected) = mixed_state();

    let (status, _) = rename(
        state.clone(),
        store_id.clone(),
        resource_id.clone(),
        "files",
        "config/missing.toml",
        "config/new.toml",
        None,
    );
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_resource(&state, &store_id, &resource_id, &expected);
}

#[test]
fn admin_secret_rename_conflict_leaves_resource_unchanged() {
    let (_tmp, state, store_id, resource_id, expected) = mixed_state();

    let (status, body) = rename(
        state.clone(),
        store_id.clone(),
        resource_id.clone(),
        "env",
        "PRIMARY_TOKEN",
        "KEEP_TOKEN",
        None,
    );
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "target_key_exists");
    assert_resource(&state, &store_id, &resource_id, &expected);
}

fn assert_unknown_resource(status: StatusCode, body: &serde_json::Value) {
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "not_found");
    assert_eq!(body["message"], "unknown resource");
    assert!(!body.to_string().contains("res_missing"));
}

#[test]
fn admin_reference_every_explicit_flat_route_rejects_before_blob_access() {
    let store = Arc::new(TestStore::new());
    let state = Arc::new(test_admin_state(std::collections::HashMap::from([(
        "store".to_string(),
        store.clone() as Arc<dyn StorageBackend>,
    )])));
    let resource_id = "res_missing".to_string();

    let calls = vec![
        run_handler(handle_admin_secrets_list(
            State(state.clone()),
            Path("store".to_string()),
            Query(SecretListQuery {
                resource_id: Some(resource_id.clone()),
            }),
        )),
        run_handler(handle_admin_secret_read(
            State(state.clone()),
            Path((
                "store".to_string(),
                resource_id.clone(),
                "env".to_string(),
                "TOKEN".to_string(),
            )),
        )),
        run_handler(handle_admin_secret_write(
            State(state.clone()),
            Extension(principal()),
            Path((
                "store".to_string(),
                resource_id.clone(),
                "env".to_string(),
                "TOKEN".to_string(),
            )),
            ResourceBody(underwrap_protocol::service::AdminSecretWriteRequest {
                material_b64: base64::engine::general_purpose::STANDARD.encode(b"synthetic"),
            }),
        )),
        run_handler(handle_admin_secret_delete(
            State(state.clone()),
            Extension(principal()),
            Path((
                "store".to_string(),
                resource_id.clone(),
                "files".to_string(),
                "path".to_string(),
            )),
        )),
        rename(
            state,
            "store".to_string(),
            resource_id,
            "env",
            "TOKEN",
            "RENAMED",
            Some(b"synthetic"),
        ),
    ];

    for (status, body) in &calls {
        assert_unknown_resource(*status, body);
    }
    assert_eq!(
        store.operation_trace(),
        vec!["read:control/resources".to_string(); calls.len()],
        "missing-resource routes must neither list nor access resources/<id>",
    );
    assert_eq!(store.data.lock().unwrap().len(), 0);
}

#[test]
fn admin_reference_existing_resource_keeps_missing_flat_content_semantics() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let resource_id = create_resource_id_for_test(
        state.clone(),
        &admin_principal(),
        &store_id,
        "empty/resource",
    );
    let (read_status, read_body) = run_handler(handle_admin_secret_read(
        State(state.clone()),
        Path((
            store_id.clone(),
            resource_id.clone(),
            "env".to_string(),
            "MISSING".to_string(),
        )),
    ));
    assert_eq!(read_status, StatusCode::OK);
    assert_eq!(read_body["material_b64"], serde_json::Value::Null);

    let (delete_status, delete_body) = run_handler(handle_admin_secret_delete(
        State(state),
        Extension(principal()),
        Path((
            store_id,
            resource_id,
            "env".to_string(),
            "MISSING".to_string(),
        )),
    ));
    assert_eq!(delete_status, StatusCode::OK);
    assert_eq!(delete_body["existed"], false);
}
