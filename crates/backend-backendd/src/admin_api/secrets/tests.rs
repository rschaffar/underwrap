use axum::Extension;
use axum::extract::{Path, State};
use base64::Engine;
use hyper::StatusCode;

use super::*;
use crate::http::responses::Body;
use crate::storage::secret_entries::list_secret_entries;
use crate::test_support::{
    admin_principal, create_resource_id_for_test, entity_test_state_with_store, run_handler,
};

fn principal() -> crate::users::Principal {
    crate::users::Principal {
        id: "alice-id".into(),
        username: "alice".into(),
    }
}

#[test]
fn admin_secret_rename_moves_entry_atomically() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();
    let resource_id = create_resource_id_for_test(state.clone(), &p, &store_id, "github/token");

    // Seed an entry.
    let (status, _) = run_handler(handle_admin_secret_write(
        State(state.clone()),
        Extension(principal()),
        Path((
            store_id.clone(),
            resource_id.clone(),
            "files".into(),
            "config.yaml".into(),
        )),
        Body(underwrap_protocol::service::AdminSecretWriteRequest {
            material_b64: base64::engine::general_purpose::STANDARD.encode("v1"),
        }),
    ));
    assert_eq!(status, StatusCode::OK);

    // Rename without changing material.
    let (rename_status, _) = run_handler(handle_admin_secret_rename(
        State(state.clone()),
        Extension(principal()),
        Path((
            store_id.clone(),
            resource_id.clone(),
            "files".into(),
            "config.yaml".into(),
        )),
        Body(underwrap_protocol::service::AdminSecretRenameRequest {
            to_key: "config.json".into(),
            material_b64: None,
        }),
    ));
    assert_eq!(rename_status, StatusCode::OK);

    // The new key holds the original material; the old key is gone.
    let entries = {
        let backend = state.stores.read().unwrap().expect_unlocked(&store_id);
        list_secret_entries(backend.as_ref(), None).unwrap()
    };
    let keys: Vec<&str> = entries.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(keys, vec!["config.json"]);
}

#[test]
fn admin_secret_rename_refuses_when_target_exists() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();
    let resource_id = create_resource_id_for_test(state.clone(), &p, &store_id, "r");
    for k in ["a", "b"] {
        let (status, _) = run_handler(handle_admin_secret_write(
            State(state.clone()),
            Extension(principal()),
            Path((
                store_id.clone(),
                resource_id.clone(),
                "files".into(),
                k.into(),
            )),
            Body(underwrap_protocol::service::AdminSecretWriteRequest {
                material_b64: base64::engine::general_purpose::STANDARD.encode(k),
            }),
        ));
        assert_eq!(status, StatusCode::OK);
    }
    let (status, body) = run_handler(handle_admin_secret_rename(
        State(state.clone()),
        Extension(principal()),
        Path((store_id, resource_id, "files".into(), "a".into())),
        Body(underwrap_protocol::service::AdminSecretRenameRequest {
            to_key: "b".into(),
            material_b64: None,
        }),
    ));
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "target_key_exists");
}
