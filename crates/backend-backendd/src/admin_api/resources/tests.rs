use super::*;
use crate::admin_api::pairs::handle_admin_pair_create;
use crate::storage::documents::save_policy_document;
use crate::storage::secret_entries::write_resource_entry;
use crate::test_support::{
    admin_principal, create_resource_id_for_test, entity_test_state_with_store, response_body_json,
};

#[test]
fn admin_resource_create_then_list_round_trip_with_operator_provenance() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();

    let create = handle_admin_resource_create(
        &state,
        &p,
        &underwrap_protocol::service::AdminResourceCreateRequest {
            store: store_id.clone(),
            name: "github/token".into(),
        },
    );
    assert_eq!(create.status(), StatusCode::OK);
    let body = response_body_json(create);
    assert_eq!(body["resource"]["name"], "github/token");
    assert_eq!(body["resource"]["created_by"]["kind"], "operator");
    assert_eq!(body["resource"]["created_by"]["username"], "alice");

    let list = handle_admin_resources_list(
        &state,
        &underwrap_protocol::service::AdminResourcesListRequest {
            store: store_id.clone(),
        },
    );
    let list_body = response_body_json(list);
    let resources = list_body["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0]["name"], "github/token");
}

#[test]
fn admin_resource_create_refuses_duplicate_name() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();
    let body = underwrap_protocol::service::AdminResourceCreateRequest {
        store: store_id,
        name: "github/token".into(),
    };
    let first = handle_admin_resource_create(&state, &p, &body);
    assert_eq!(first.status(), StatusCode::OK);
    let dup = handle_admin_resource_create(&state, &p, &body);
    assert_eq!(dup.status(), StatusCode::CONFLICT);
    assert_eq!(response_body_json(dup)["code"], "resource_already_exists");
}

#[test]
fn admin_resource_delete_succeeds_when_no_dependents() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();
    let resource_id = create_resource_id_for_test(&state, &p, &store_id, "github/token");
    let del = handle_admin_resource_delete(
        &state,
        &p,
        &underwrap_protocol::service::AdminResourceDeleteRequest {
            store: store_id.clone(),
            id: resource_id,
        },
    );
    assert_eq!(del.status(), StatusCode::OK);
    let body = response_body_json(del);
    assert_eq!(body["ok"], true);
    assert!(body.get("blocked_by").is_none() || body["blocked_by"].is_null());

    let list = handle_admin_resources_list(
        &state,
        &underwrap_protocol::service::AdminResourcesListRequest { store: store_id },
    );
    assert_eq!(
        response_body_json(list)["resources"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn admin_resource_rename_flips_name_and_keeps_id() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();
    let resource_id = create_resource_id_for_test(&state, &p, &store_id, "github/token");

    let resp = handle_admin_resource_rename(
        &state,
        &p,
        &underwrap_protocol::service::AdminResourceRenameRequest {
            store: store_id.clone(),
            id: resource_id.clone(),
            new_name: "github/main".into(),
        },
    );
    assert_eq!(resp.status(), StatusCode::OK);
    let body = response_body_json(resp);
    assert_eq!(body["resource"]["id"], resource_id);
    assert_eq!(body["resource"]["name"], "github/main");

    // List confirms the rename.
    let list = handle_admin_resources_list(
        &state,
        &underwrap_protocol::service::AdminResourcesListRequest { store: store_id },
    );
    let resources = response_body_json(list)["resources"].clone();
    assert_eq!(resources[0]["id"], resource_id);
    assert_eq!(resources[0]["name"], "github/main");
}

#[test]
fn admin_resource_rename_refuses_when_target_name_taken() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();
    let a = create_resource_id_for_test(&state, &p, &store_id, "a");
    let _b = create_resource_id_for_test(&state, &p, &store_id, "b");

    let resp = handle_admin_resource_rename(
        &state,
        &p,
        &underwrap_protocol::service::AdminResourceRenameRequest {
            store: store_id.clone(),
            id: a,
            new_name: "b".into(),
        },
    );
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    assert_eq!(response_body_json(resp)["code"], "resource_already_exists");
}

#[test]
fn admin_resource_delete_refuses_with_structured_blockers() {
    use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();

    // Resource exists.
    let resource_id = create_resource_id_for_test(&state, &p, &store_id, "github/token");
    // Pair references it.
    handle_admin_pair_create(
        &state,
        &p,
        &underwrap_protocol::service::AdminPairCreateRequest {
            store: store_id.clone(),
            wrapper: "gh".into(),
            resource_id: resource_id.clone(),
        },
    );
    // A secret under the resource. The per-slot in-blob entry key
    // shape routes through the blob shim and ends up inside the
    // `resources/<resource_id>` blob.
    {
        let reg = state.stores.read().unwrap();
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
        let reg = state.stores.read().unwrap();
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

    let del = handle_admin_resource_delete(
        &state,
        &p,
        &underwrap_protocol::service::AdminResourceDeleteRequest {
            store: store_id.clone(),
            id: resource_id.clone(),
        },
    );
    assert_eq!(del.status(), StatusCode::OK);
    let body = response_body_json(del);
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
    let list = handle_admin_resources_list(
        &state,
        &underwrap_protocol::service::AdminResourcesListRequest { store: store_id },
    );
    assert_eq!(
        response_body_json(list)["resources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
