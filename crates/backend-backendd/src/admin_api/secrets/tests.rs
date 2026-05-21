use super::*;
use crate::storage::secret_entries::list_secret_entries;
use crate::test_support::{
    admin_principal, create_resource_id_for_test, entity_test_state_with_store, response_body_json,
};
use base64::Engine;

#[test]
fn admin_secret_rename_moves_entry_atomically() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal(&[]);
    let resource_id = create_resource_id_for_test(&state, &p, &store_id, "github/token");

    // Seed an entry.
    let write = handle_admin_secret_write(
        &state,
        "alice",
        underwrap_protocol::service::AdminSecretWriteRequest {
            store: store_id.clone(),
            resource_id: resource_id.clone(),
            slot: underwrap_protocol::resource::ResourceSlot::Files,
            key: "config.yaml".into(),
            material_b64: base64::engine::general_purpose::STANDARD.encode("v1"),
        },
    );
    assert_eq!(write.status(), StatusCode::OK);

    // Rename without changing material.
    let rename = handle_admin_secret_rename(
        &state,
        "alice",
        underwrap_protocol::service::AdminSecretRenameRequest {
            store: store_id.clone(),
            resource_id: resource_id.clone(),
            slot: underwrap_protocol::resource::ResourceSlot::Files,
            from_key: "config.yaml".into(),
            to_key: "config.json".into(),
            material_b64: None,
        },
    );
    assert_eq!(rename.status(), StatusCode::OK);

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
    let p = admin_principal(&[]);
    let resource_id = create_resource_id_for_test(&state, &p, &store_id, "r");
    for k in ["a", "b"] {
        let _ = handle_admin_secret_write(
            &state,
            "alice",
            underwrap_protocol::service::AdminSecretWriteRequest {
                store: store_id.clone(),
                resource_id: resource_id.clone(),
                slot: underwrap_protocol::resource::ResourceSlot::Files,
                key: k.into(),
                material_b64: base64::engine::general_purpose::STANDARD.encode(k),
            },
        );
    }
    let resp = handle_admin_secret_rename(
        &state,
        "alice",
        underwrap_protocol::service::AdminSecretRenameRequest {
            store: store_id.clone(),
            resource_id: resource_id.clone(),
            slot: underwrap_protocol::resource::ResourceSlot::Files,
            from_key: "a".into(),
            to_key: "b".into(),
            material_b64: None,
        },
    );
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    assert_eq!(response_body_json(resp)["code"], "target_key_exists");
}
