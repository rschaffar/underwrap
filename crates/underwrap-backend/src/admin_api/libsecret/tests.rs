mod identifiers;

use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use base64::Engine;
use underwrap_backend_store::StorageBackend;

use super::*;
use crate::http::responses::{Body, ResourceBody};
use crate::test_support::{
    TestStore, admin_principal, create_resource_id_for_test, entity_test_state_with_store,
    run_handler, test_admin_state,
};

#[test]
fn validate_segment_matches_publication_grammar() {
    for id in ["default", "item_1", "A1", "1", "_meta", &"a".repeat(128)] {
        validate_segment("collection", id).expect("valid collection segment");
        validate_segment("alias", id).expect("valid alias segment");
    }
    for id in [
        "",
        "gh-cli",
        "a.b",
        ".",
        "..",
        "a/b",
        "a b",
        "é",
        &"a".repeat(129),
    ] {
        let err = validate_segment("collection", id).unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    }
}

#[test]
fn validate_item_id_matches_publication_grammar() {
    for id in ["0".repeat(32), "abcdef0123456789".repeat(2)] {
        validate_item_id(&id).expect("valid item id");
    }
    for id in [
        "".into(),
        "item_1".into(),
        "a".repeat(31),
        "a".repeat(33),
        "A".repeat(32),
        "g".repeat(32),
        "_".repeat(32),
        "é".repeat(16),
    ] {
        let err = validate_item_id(&id).unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    }
}

#[test]
fn next_auto_id_picks_first_unused() {
    let mut existing: BTreeMap<String, ()> = BTreeMap::new();
    assert_eq!(next_auto_id("c", &existing), "c1");
    existing.insert("c1".into(), ());
    assert_eq!(next_auto_id("c", &existing), "c2");
    existing.insert("c3".into(), ());
    // Collection gaps are filled: smallest free slot is c2.
    assert_eq!(next_auto_id("c", &existing), "c2");
}

fn assert_unknown_resource(status: StatusCode, body: &serde_json::Value) {
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "not_found");
    assert_eq!(body["message"], "unknown resource");
    assert!(!body.to_string().contains("res_missing"));
}

#[test]
fn admin_reference_all_eight_libsecret_operations_require_parent_before_blob_access() {
    let store = Arc::new(TestStore::new());
    let state = Arc::new(test_admin_state(std::collections::HashMap::from([(
        "store".to_string(),
        store.clone() as Arc<dyn StorageBackend>,
    )])));
    let resource_id = "res_missing".to_string();
    let principal = admin_principal();

    let calls = vec![
        run_handler(handle_get_tree(
            State(state.clone()),
            Path(("store".to_string(), resource_id.clone())),
        )),
        run_handler(handle_post_collection(
            State(state.clone()),
            Extension(principal.clone()),
            Path(("store".to_string(), resource_id.clone())),
            Body(AdminLibsecretCollectionCreateRequest {
                id: Some("default".into()),
                label: "Default".into(),
            }),
        )),
        run_handler(handle_delete_collection(
            State(state.clone()),
            Extension(principal.clone()),
            Path((
                "store".to_string(),
                resource_id.clone(),
                "default".to_string(),
            )),
        )),
        run_handler(handle_post_item(
            State(state.clone()),
            Extension(principal.clone()),
            Path((
                "store".to_string(),
                resource_id.clone(),
                "default".to_string(),
            )),
            ResourceBody(AdminLibsecretItemCreateRequest {
                id: Some("item".into()),
                label: "Item".into(),
                content_type: "text/plain".into(),
                attributes: BTreeMap::new(),
                secret_b64: base64::engine::general_purpose::STANDARD.encode(b"synthetic"),
            }),
        )),
        run_handler(handle_delete_item(
            State(state.clone()),
            Extension(principal.clone()),
            Path((
                "store".to_string(),
                resource_id.clone(),
                "default".to_string(),
                "item".to_string(),
            )),
        )),
        run_handler(handle_get_item_secret(
            State(state.clone()),
            Path((
                "store".to_string(),
                resource_id.clone(),
                "default".to_string(),
                "item".to_string(),
            )),
        )),
        run_handler(handle_put_alias(
            State(state.clone()),
            Extension(principal.clone()),
            Path((
                "store".to_string(),
                resource_id.clone(),
                "default".to_string(),
            )),
            Body(AdminLibsecretAliasUpsertRequest {
                collection_id: "default".into(),
            }),
        )),
        run_handler(handle_delete_alias(
            State(state),
            Extension(principal),
            Path(("store".to_string(), resource_id, "default".to_string())),
        )),
    ];

    for (status, body) in &calls {
        assert_unknown_resource(*status, body);
    }
    assert_eq!(
        store.operation_trace(),
        vec!["read:control/resources".to_string(); calls.len()],
        "shared read/mutation seams must not access resources/<id>",
    );
    assert!(store.data.lock().unwrap().is_empty());
}

#[test]
fn admin_reference_existing_empty_resource_preserves_subordinate_semantics() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let resource_id = create_resource_id_for_test(
        state.clone(),
        &admin_principal(),
        &store_id,
        "empty/libsecret",
    );

    let (tree_status, tree) = run_handler(handle_get_tree(
        State(state.clone()),
        Path((store_id.clone(), resource_id.clone())),
    ));
    assert_eq!(tree_status, StatusCode::OK);
    assert_eq!(tree["aliases"], serde_json::json!({}));
    assert_eq!(tree["collections"], serde_json::json!([]));

    let (collection_status, collection) = run_handler(handle_delete_collection(
        State(state.clone()),
        Extension(admin_principal()),
        Path((store_id.clone(), resource_id.clone(), "missing".to_string())),
    ));
    assert_eq!(collection_status, StatusCode::OK);
    assert_eq!(collection["existed"], false);

    let (alias_status, alias) = run_handler(handle_delete_alias(
        State(state.clone()),
        Extension(admin_principal()),
        Path((store_id.clone(), resource_id.clone(), "missing".to_string())),
    ));
    assert_eq!(alias_status, StatusCode::OK);
    assert_eq!(alias["existed"], false);

    let (item_status, item) = run_handler(handle_delete_item(
        State(state),
        Extension(admin_principal()),
        Path((
            store_id,
            resource_id,
            "missing".to_string(),
            "item".to_string(),
        )),
    ));
    assert_eq!(item_status, StatusCode::NOT_FOUND);
    assert_eq!(item["code"], "not_found");
    assert_eq!(item["message"], "collection 'missing'");
}
