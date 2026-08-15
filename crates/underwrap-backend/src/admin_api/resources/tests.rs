use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, State};
use hyper::StatusCode;
use underwrap_backend_store::{EncryptedBackend, StorageBackend, save_resource_blob};
use underwrap_protocol::entities::{EntityCreator, ResourceRecord, ResourcesDocument};
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
use underwrap_protocol::resource::resource_blob_key;
use underwrap_protocol::resource_blob::{
    Collection, Item, MAX_CANONICAL_RESOURCE_BYTES, ResourceBlob,
};

use super::*;
use crate::admin_api::csrf::Principal;
use crate::admin_api::pairs::handle_admin_pair_create;
use crate::http::responses::Body;
use crate::runtime_state::ListenerState;
use crate::storage::documents::save_policy_document;
use crate::storage::secret_entries::write_resource_entry;
use crate::test_support::{
    TestStore, admin_principal, capture_tracing_output, create_resource_id_for_test,
    entity_test_state_with_store, run_handler, test_admin_state,
};

fn entity_state() -> (tempfile::TempDir, Arc<ListenerState>, String, Principal) {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    (tmp, state, store_id, admin_principal())
}

fn create_resource(
    state: Arc<ListenerState>,
    principal: Principal,
    store_id: String,
    name: &str,
) -> (StatusCode, serde_json::Value) {
    run_handler(handle_admin_resource_create(
        State(state),
        Extension(principal),
        Path(store_id),
        Body(underwrap_protocol::service::AdminResourceCreateRequest { name: name.into() }),
    ))
}

fn rename_resource(
    state: Arc<ListenerState>,
    principal: Principal,
    store_id: String,
    resource_id: String,
    new_name: &str,
) -> (StatusCode, serde_json::Value) {
    run_handler(handle_admin_resource_rename(
        State(state),
        Extension(principal),
        Path((store_id, resource_id)),
        Body(underwrap_protocol::service::AdminResourceRenameRequest {
            new_name: new_name.into(),
        }),
    ))
}

fn state_with_test_store() -> (Arc<ListenerState>, String, Arc<TestStore>) {
    let store = Arc::new(TestStore::new());
    let store_id = "work".to_string();
    let state = Arc::new(test_admin_state(HashMap::from([(
        store_id.clone(),
        store.clone() as Arc<dyn StorageBackend>,
    )])));
    (state, store_id, store)
}

fn seed_resource(store: &dyn StorageBackend, resource_id: &str) {
    let mut resources = ResourcesDocument::default();
    resources
        .insert(ResourceRecord {
            id: resource_id.into(),
            name: "resource/name".into(),
            created_by: EntityCreator::Operator {
                id: "admin-id".into(),
                username: "alice".into(),
            },
            created_at: 1,
        })
        .unwrap();
    crate::storage::documents::save_resources_document(store, &resources).unwrap();
}

fn delete_resource(
    state: Arc<ListenerState>,
    store_id: &str,
    resource_id: &str,
) -> (StatusCode, serde_json::Value) {
    run_handler(handle_admin_resource_delete(
        State(state),
        Extension(admin_principal()),
        Path((store_id.to_string(), resource_id.to_string())),
    ))
}

fn item_fixture() -> Item {
    Item {
        label: "label-must-not-copy".into(),
        content_type: "content-type-must-not-copy".into(),
        created: 10,
        modified: 11,
        attributes: BTreeMap::from([("attribute-must-not-copy".into(), "value".into())]),
        secret: SecretBytes::from("item-secret-must-not-copy"),
    }
}

fn collection(items: BTreeMap<String, Item>) -> Collection {
    Collection {
        label: "collection-label-must-not-copy".into(),
        created: 12,
        modified: 13,
        items,
    }
}

fn capture_audit(run: impl FnOnce()) -> String {
    capture_tracing_output(run).1
}

#[test]
fn admin_resource_create_then_list_round_trip_with_operator_provenance() {
    let (_tmp, state, store_id, p) = entity_state();
    let (status, body) = create_resource(state.clone(), p, store_id.clone(), "github/token");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["resource"]["name"], "github/token");
    assert_eq!(body["resource"]["created_by"]["kind"], "operator");
    assert_eq!(body["resource"]["created_by"]["username"], "alice");

    let (status, body) = run_handler(handle_admin_resources_list(State(state), Path(store_id)));
    assert_eq!(status, StatusCode::OK);
    let resources = body["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0]["name"], "github/token");
}

#[test]
fn admin_resource_create_refuses_duplicate_name() {
    let (_tmp, state, store_id, p) = entity_state();
    let (status, _) = create_resource(state.clone(), p.clone(), store_id.clone(), "github/token");
    assert_eq!(status, StatusCode::OK);
    let (status, body) = create_resource(state, p, store_id, "github/token");
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "resource_already_exists");
}

#[test]
fn admin_resource_rename_flips_name_and_keeps_id() {
    let (_tmp, state, store_id, p) = entity_state();
    let resource_id = create_resource_id_for_test(state.clone(), &p, &store_id, "github/token");
    let (status, body) = rename_resource(
        state.clone(),
        p,
        store_id.clone(),
        resource_id.clone(),
        "github/main",
    );
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["resource"]["id"], resource_id);
    assert_eq!(body["resource"]["name"], "github/main");

    let (_, body) = run_handler(handle_admin_resources_list(State(state), Path(store_id)));
    assert_eq!(body["resources"][0]["id"], resource_id);
    assert_eq!(body["resources"][0]["name"], "github/main");
}

#[test]
fn admin_resource_rename_refuses_when_target_name_taken() {
    let (_tmp, state, store_id, p) = entity_state();
    let a = create_resource_id_for_test(state.clone(), &p, &store_id, "a");
    create_resource_id_for_test(state.clone(), &p, &store_id, "b");
    let (status, body) = rename_resource(state, p, store_id, a, "b");
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "resource_already_exists");
}

#[test]
fn admin_resource_delete_succeeds_when_no_dependents() {
    let (_tmp, state, store_id, p) = entity_state();
    let resource_id = create_resource_id_for_test(state.clone(), &p, &store_id, "github/token");
    let (status, body) = run_handler(handle_admin_resource_delete(
        State(state.clone()),
        Extension(p),
        Path((store_id.clone(), resource_id)),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert!(body.get("blocked_by").is_none() || body["blocked_by"].is_null());
    let (status, body) = run_handler(handle_admin_resources_list(State(state), Path(store_id)));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["resources"].as_array().unwrap().len(), 0);
}

#[test]
fn admin_resource_delete_refuses_with_structured_blockers() {
    let (_tmp, state, store_id, p) = entity_state();
    let resource_id = create_resource_id_for_test(state.clone(), &p, &store_id, "github/token");
    let (status, _) = run_handler(handle_admin_pair_create(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
        Body(underwrap_protocol::service::AdminPairCreateRequest {
            wrapper: "gh".into(),
            resource_id: resource_id.clone(),
        }),
    ));
    assert_eq!(status, StatusCode::OK);
    {
        let stores = state.shared.stores.read().unwrap();
        let store = stores.expect_unlocked(&store_id);
        write_resource_entry(
            store.as_ref(),
            &format!("{resource_id}/env/GH_TOKEN"),
            b"ghp_value",
        )
        .unwrap();
        save_policy_document(
            store.as_ref(),
            &PolicyDocument {
                permissions: vec![WrapperPermission {
                    client_instance: "inst_a".into(),
                    wrapper: "gh".into(),
                    resource_id: resource_id.clone(),
                    mode: WrapperApprovalMode::Unlocked,
                    writable: false,
                    reason: None,
                }],
            },
        )
        .unwrap();
    }
    let (status, body) = delete_resource(state.clone(), &store_id, &resource_id);
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], false);
    let blockers = &body["blocked_by"];
    assert_eq!(blockers["content_entry_count"], 1);
    assert_eq!(
        blockers["sample_content"][0],
        serde_json::json!({"kind": "env", "name": "GH_TOKEN"})
    );
    assert!(blockers.get("secret_count").is_none());
    assert!(blockers.get("sample_secret_ids").is_none());
    assert_eq!(blockers["pair_count"], 1);
    assert_eq!(blockers["permission_count"], 1);
    assert_eq!(blockers["sample_pairs"][0]["wrapper"], "gh");
    assert_eq!(
        blockers["sample_permissions"][0]["client_instance"],
        "inst_a"
    );
    let (_, body) = run_handler(handle_admin_resources_list(State(state), Path(store_id)));
    assert_eq!(body["resources"].as_array().unwrap().len(), 1);
}

fn assert_excludes(rendered: &str, excluded: &[&str]) {
    for value in excluded {
        assert!(!rendered.contains(value), "response leaked {value}");
    }
}

#[rustfmt::skip]
fn assert_content_blocked(index: usize, blob: ResourceBlob, count: usize, sample: serde_json::Value) {
    let (state, store_id, store) = state_with_test_store();
    let resource_id = format!("res_case_{index}");
    seed_resource(store.as_ref(), &resource_id);
    save_resource_blob(store.as_ref(), &resource_id, &blob).unwrap();
    let before = store.data.lock().unwrap().clone();
    let (status, body) = delete_resource(state, &store_id, &resource_id);
    assert_eq!(status, StatusCode::OK, "case {index}: {body}");
    assert_eq!(body["ok"], false);
    assert_eq!(body["blocked_by"]["content_entry_count"], count, "case {index}: {body}");
    assert_eq!(body["blocked_by"]["sample_content"], sample, "case {index}: {body}");
    assert_eq!(*store.data.lock().unwrap(), before, "case {index} mutated");
    assert_excludes(&body.to_string(), &[
        "env-value-must-not-copy", "file-value-must-not-copy", "missing-target-must-not-copy",
        "collection-label-must-not-copy", "label-must-not-copy", "content-type-must-not-copy",
        "attribute-must-not-copy", "item-secret-must-not-copy",
    ]);
}

#[test]
#[rustfmt::skip]
fn admin_resource_delete_enumerates_every_content_kind_without_values_or_validation() {
    let mut blob = ResourceBlob::empty();
    blob.env.insert("ENV_ID".into(), SecretBytes::from("env-value-must-not-copy"));
    assert_content_blocked(0, blob, 1, serde_json::json!([{"kind": "env", "name": "ENV_ID"}]));

    let mut blob = ResourceBlob::empty();
    blob.files.insert("private/path".into(), SecretBytes::from("file-value-must-not-copy"));
    assert_content_blocked(1, blob, 1, serde_json::json!([{"kind": "file", "path": "private/path"}]));

    let mut blob = ResourceBlob::empty();
    blob.libsecret.aliases.insert("dangling".into(), "missing-target-must-not-copy".into());
    assert_content_blocked(2, blob, 1, serde_json::json!([{"kind": "libsecret_alias", "name": "dangling"}]));

    let mut blob = ResourceBlob::empty();
    blob.libsecret.collections.insert("empty".into(), collection(BTreeMap::new()));
    assert_content_blocked(3, blob, 1, serde_json::json!([
        {"kind": "libsecret_collection", "collection_id": "empty"}
    ]));

    let mut blob = ResourceBlob::empty();
    blob.libsecret.collections.insert(
        "collection".into(), collection(BTreeMap::from([("item".into(), item_fixture())])),
    );
    assert_content_blocked(4, blob, 2, serde_json::json!([
        {"kind": "libsecret_collection", "collection_id": "collection"},
        {"kind": "libsecret_item", "collection_id": "collection", "item_id": "item"}
    ]));
}

#[test]
#[rustfmt::skip]
fn admin_resource_delete_mixed_count_is_exact_and_sample_is_capped_without_values() {
    let (state, store_id, store) = state_with_test_store();
    let resource_id = "res_mixed";
    seed_resource(store.as_ref(), resource_id);
    let mut blob = ResourceBlob::empty();
    blob.env.insert("A".into(), SecretBytes::from("value-a"));
    blob.env.insert("B".into(), SecretBytes::from("value-b"));
    blob.files.insert("path".into(), SecretBytes::from("bytes"));
    blob.libsecret.aliases.insert("alias".into(), "target".into());
    blob.libsecret.collections.insert("collection".into(), Collection {
        label: "label".into(), created: 1, modified: 2,
        items: BTreeMap::from([("item".into(), Item {
            label: "item-label".into(), content_type: "text/plain".into(), created: 3, modified: 4,
            attributes: BTreeMap::new(), secret: SecretBytes::from("secret-bytes"),
        })]),
    });
    save_resource_blob(store.as_ref(), resource_id, &blob).unwrap();
    let (status, body) = delete_resource(state, &store_id, resource_id);
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["blocked_by"]["content_entry_count"], 6);
    assert_eq!(body["blocked_by"]["sample_content"].as_array().unwrap().len(), ENTITY_DEPENDENT_SAMPLE_LIMIT);
    assert_eq!(body["blocked_by"]["sample_content"], serde_json::json!([
        {"kind": "env", "name": "A"},
        {"kind": "env", "name": "B"},
        {"kind": "file", "path": "path"},
        {"kind": "libsecret_alias", "name": "alias"},
        {"kind": "libsecret_collection", "collection_id": "collection"}
    ]));
    assert_excludes(&body.to_string(), &[
        "value-a", "value-b", "bytes", "target", "item-label", "secret-bytes",
    ]);
}

#[test]
fn admin_resource_delete_missing_and_persisted_empty_blobs_remove_catalog_and_key_in_order() {
    for persisted in [false, true] {
        let (state, store_id, store) = state_with_test_store();
        let resource_id = if persisted {
            "res_empty"
        } else {
            "res_missing"
        };
        seed_resource(store.as_ref(), resource_id);
        if persisted {
            save_resource_blob(store.as_ref(), resource_id, &ResourceBlob::empty()).unwrap();
        }
        store.clear_operation_trace();
        let (status, body) = delete_resource(state, &store_id, resource_id);
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["ok"], true);
        assert_eq!(store.read(&resource_blob_key(resource_id)).unwrap(), None);
        assert!(
            crate::storage::documents::load_resources_document(store.as_ref())
                .unwrap()
                .find_by_id(resource_id)
                .is_none()
        );
        let trace = store.operation_trace();
        let delete_at = trace
            .iter()
            .position(|op| op == &format!("delete:{}", resource_blob_key(resource_id)))
            .unwrap();
        let catalog_write_at = trace
            .iter()
            .position(|op| op == "write:control/resources")
            .unwrap();
        assert!(delete_at < catalog_write_at, "unexpected trace: {trace:?}");
    }
}

#[test]
fn admin_resource_delete_invalid_active_blobs_fail_before_mutation_or_later_calls() {
    enum Fixture {
        Bytes(Vec<u8>),
        Unreadable,
    }
    let cases = [
        ("malformed", Fixture::Bytes(b"not-json".to_vec())),
        (
            "unsupported schema",
            Fixture::Bytes(br#"{"schema_version": 2}"#.to_vec()),
        ),
        (
            "oversized",
            Fixture::Bytes(vec![b' '; MAX_CANONICAL_RESOURCE_BYTES + 1]),
        ),
        ("unreadable", Fixture::Unreadable),
    ];
    for (label, fixture) in cases {
        let (state, store_id, store) = state_with_test_store();
        let resource_id = format!("res_{}", label.replace(' ', "_"));
        seed_resource(store.as_ref(), &resource_id);
        match fixture {
            Fixture::Bytes(bytes) => store
                .write(&resource_blob_key(&resource_id), &bytes)
                .unwrap(),
            Fixture::Unreadable => {
                save_resource_blob(store.as_ref(), &resource_id, &ResourceBlob::empty()).unwrap();
                store.fail_read_for(resource_blob_key(&resource_id));
            }
        }
        let before = store.data.lock().unwrap().clone();
        store.clear_operation_trace();
        let (status, _) = delete_resource(state, &store_id, &resource_id);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{label}");
        assert_eq!(*store.data.lock().unwrap(), before, "{label} mutated");
        assert_eq!(
            store.operation_trace(),
            vec![
                "read:control/resources".to_string(),
                format!("read:{}", resource_blob_key(&resource_id)),
            ],
            "{label} made a later call"
        );
    }
}

#[test]
fn admin_resource_delete_corrupted_encrypted_blob_fails_without_mutation() {
    let temp = tempfile::TempDir::new().unwrap();
    let store_dir = temp.path().join("encrypted");
    let store = Arc::new(EncryptedBackend::create_with_key(&store_dir, [7; 32]).unwrap());
    let resource_id = "res_corrupted_encrypted";
    seed_resource(store.as_ref(), resource_id);
    save_resource_blob(store.as_ref(), resource_id, &ResourceBlob::empty()).unwrap();
    let catalog_path = store_dir.join("control/resources");
    let active_path = store_dir.join(resource_blob_key(resource_id));
    std::fs::write(&active_path, b"corrupted encrypted material").unwrap();
    let catalog_before = std::fs::read(&catalog_path).unwrap();
    let active_before = std::fs::read(&active_path).unwrap();
    let store_id = "work".to_string();
    let state = Arc::new(test_admin_state(HashMap::from([(
        store_id.clone(),
        store as Arc<dyn StorageBackend>,
    )])));
    let (status, _) = delete_resource(state, &store_id, resource_id);
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(std::fs::read(catalog_path).unwrap(), catalog_before);
    assert_eq!(std::fs::read(active_path).unwrap(), active_before);
}

#[test]
fn admin_resource_delete_error_stops_before_catalog_write() {
    let (state, store_id, store) = state_with_test_store();
    let resource_id = "res_delete_failure";
    seed_resource(store.as_ref(), resource_id);
    save_resource_blob(store.as_ref(), resource_id, &ResourceBlob::empty()).unwrap();
    store.clear_operation_trace();
    store.fail_ops.lock().unwrap().insert("delete");
    let (status, _) = delete_resource(state, &store_id, resource_id);
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let trace = store.operation_trace();
    assert_eq!(
        trace.last(),
        Some(&format!("delete:{}", resource_blob_key(resource_id)))
    );
    assert!(!trace.iter().any(|op| op == "write:control/resources"));
    let data = store.data.lock().unwrap();
    assert!(data.contains_key(&resource_blob_key(resource_id)));
    assert!(data.contains_key("control/resources"));
}

#[test]
fn admin_resource_delete_catalog_error_does_not_reconstruct_deleted_blob() {
    let (state, store_id, store) = state_with_test_store();
    let resource_id = "res_catalog_failure";
    seed_resource(store.as_ref(), resource_id);
    save_resource_blob(store.as_ref(), resource_id, &ResourceBlob::empty()).unwrap();
    store.clear_operation_trace();
    store.fail_ops.lock().unwrap().insert("write");
    let (status, _) = delete_resource(state, &store_id, resource_id);
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let trace = store.operation_trace();
    assert_eq!(
        &trace[trace.len() - 2..],
        &[
            format!("delete:{}", resource_blob_key(resource_id)),
            "write:control/resources".to_string(),
        ]
    );
    assert_eq!(
        trace
            .iter()
            .filter(|op| *op == &format!("write:{}", resource_blob_key(resource_id)))
            .count(),
        0,
        "deleted blob was reconstructed: {trace:?}"
    );
    let data = store.data.lock().unwrap();
    assert!(!data.contains_key(&resource_blob_key(resource_id)));
    assert!(data.contains_key("control/resources"));
}

#[test]
fn blocked_delete_audit_contains_only_resource_metadata() {
    let (state, store_id, store) = state_with_test_store();
    let resource_id = "res_audit";
    seed_resource(store.as_ref(), resource_id);
    let mut blob = ResourceBlob::empty();
    blob.env.insert(
        "identifier-that-may-be-sensitive".into(),
        SecretBytes::from("value-that-must-never-appear"),
    );
    save_resource_blob(store.as_ref(), resource_id, &blob).unwrap();
    let output = capture_audit(|| {
        let (status, body) = delete_resource(state, &store_id, resource_id);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], false);
    });
    assert!(output.contains("action=resource.delete"), "{output}");
    assert!(output.contains("resource:res_audit"), "{output}");
    assert!(output.contains("result=blocked"), "{output}");
    assert!(!output.contains("identifier-that-may-be-sensitive"));
    assert!(!output.contains("value-that-must-never-appear"));
}
