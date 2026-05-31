use axum::Extension;
use axum::extract::{Path, State};
use hyper::StatusCode;

use super::*;
use crate::admin_api::pairs::handle_admin_pair_create;
use crate::http::responses::Body;
use crate::storage::documents::save_policy_document;
use crate::storage::secret_entries::write_resource_entry;
use crate::test_support::{
    admin_principal, create_resource_id_for_test, entity_test_state_with_store, run_handler,
};

#[test]
fn admin_resource_create_then_list_round_trip_with_operator_provenance() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();

    let (status, body) = run_handler(handle_admin_resource_create(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
        Body(underwrap_protocol::service::AdminResourceCreateRequest {
            name: "github/token".into(),
        }),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["resource"]["name"], "github/token");
    assert_eq!(body["resource"]["created_by"]["kind"], "operator");
    assert_eq!(body["resource"]["created_by"]["username"], "alice");

    let (list_status, list_body) = run_handler(handle_admin_resources_list(
        State(state.clone()),
        Path(store_id.clone()),
    ));
    assert_eq!(list_status, StatusCode::OK);
    let resources = list_body["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0]["name"], "github/token");
}

#[test]
fn admin_resource_create_refuses_duplicate_name() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();
    let make = || underwrap_protocol::service::AdminResourceCreateRequest {
        name: "github/token".into(),
    };
    let (first_status, _) = run_handler(handle_admin_resource_create(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
        Body(make()),
    ));
    assert_eq!(first_status, StatusCode::OK);
    let (dup_status, dup_body) = run_handler(handle_admin_resource_create(
        State(state.clone()),
        Extension(p),
        Path(store_id),
        Body(make()),
    ));
    assert_eq!(dup_status, StatusCode::CONFLICT);
    assert_eq!(dup_body["code"], "resource_already_exists");
}

#[test]
fn admin_resource_delete_succeeds_when_no_dependents() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();
    let resource_id = create_resource_id_for_test(state.clone(), &p, &store_id, "github/token");
    let (status, body) = run_handler(handle_admin_resource_delete(
        State(state.clone()),
        Extension(p),
        Path((store_id.clone(), resource_id)),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert!(body.get("blocked_by").is_none() || body["blocked_by"].is_null());

    let (list_status, list_body) = run_handler(handle_admin_resources_list(
        State(state.clone()),
        Path(store_id),
    ));
    assert_eq!(list_status, StatusCode::OK);
    assert_eq!(list_body["resources"].as_array().unwrap().len(), 0);
}

#[test]
fn admin_resource_rename_flips_name_and_keeps_id() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();
    let resource_id = create_resource_id_for_test(state.clone(), &p, &store_id, "github/token");

    let (status, body) = run_handler(handle_admin_resource_rename(
        State(state.clone()),
        Extension(p),
        Path((store_id.clone(), resource_id.clone())),
        Body(underwrap_protocol::service::AdminResourceRenameRequest {
            new_name: "github/main".into(),
        }),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["resource"]["id"], resource_id);
    assert_eq!(body["resource"]["name"], "github/main");

    // List confirms the rename.
    let (_, list_body) = run_handler(handle_admin_resources_list(
        State(state.clone()),
        Path(store_id),
    ));
    let resources = list_body["resources"].clone();
    assert_eq!(resources[0]["id"], resource_id);
    assert_eq!(resources[0]["name"], "github/main");
}

#[test]
fn admin_resource_rename_refuses_when_target_name_taken() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();
    let a = create_resource_id_for_test(state.clone(), &p, &store_id, "a");
    let _b = create_resource_id_for_test(state.clone(), &p, &store_id, "b");

    let (status, body) = run_handler(handle_admin_resource_rename(
        State(state.clone()),
        Extension(p),
        Path((store_id, a)),
        Body(underwrap_protocol::service::AdminResourceRenameRequest {
            new_name: "b".into(),
        }),
    ));
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "resource_already_exists");
}

#[test]
fn admin_resource_delete_refuses_with_structured_blockers() {
    use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();

    // Resource exists.
    let resource_id = create_resource_id_for_test(state.clone(), &p, &store_id, "github/token");
    // Pair references it.
    let (pair_status, _) = run_handler(handle_admin_pair_create(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
        Body(underwrap_protocol::service::AdminPairCreateRequest {
            wrapper: "gh".into(),
            resource_id: resource_id.clone(),
        }),
    ));
    assert_eq!(pair_status, StatusCode::OK);
    // A secret under the resource. The per-slot in-blob entry key
    // shape routes through the blob shim and ends up inside the
    // `resources/<resource_id>` blob.
    {
        let reg = state.shared.stores.read().unwrap();
        let blob_key = format!("{}/env/GH_TOKEN", resource_id);
        write_resource_entry(
            reg.expect_unlocked(&store_id).as_ref(),
            &blob_key,
            b"ghp_value",
        )
        .unwrap();
    }
    // A wrapper permission referencing the resource.
    {
        let reg = state.shared.stores.read().unwrap();
        save_policy_document(
            reg.expect_unlocked(&store_id).as_ref(),
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

    let (status, body) = run_handler(handle_admin_resource_delete(
        State(state.clone()),
        Extension(p),
        Path((store_id.clone(), resource_id.clone())),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], false);
    let blockers = &body["blocked_by"];
    assert_eq!(blockers["secret_count"], 1);
    assert_eq!(blockers["pair_count"], 1);
    assert_eq!(blockers["permission_count"], 1);
    assert_eq!(blockers["sample_pairs"][0]["wrapper"], "gh");
    assert_eq!(
        blockers["sample_permissions"][0]["client_instance"],
        "inst_a"
    );

    // Resource is still present.
    let (_, list_body) = run_handler(handle_admin_resources_list(
        State(state.clone()),
        Path(store_id),
    ));
    let resources = list_body["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 1);
}
