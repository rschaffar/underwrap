use super::*;
use crate::storage::documents::save_policy_document;
use crate::test_support::{
    admin_principal, create_resource_id_for_test, entity_test_state_with_store, response_body_json,
};

#[test]
fn admin_pair_create_requires_existing_resource_and_refuses_duplicates() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal(&[]);

    // No resource → 400 unknown_resource.
    let missing = handle_admin_pair_create(
        &state,
        &p,
        &underwrap_protocol::service::AdminPairCreateRequest {
            store: store_id.clone(),
            wrapper: "gh".into(),
            resource_id: "res_missing".into(),
        },
    );
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response_body_json(missing)["code"], "unknown_resource");

    let resource_id = create_resource_id_for_test(&state, &p, &store_id, "github/token");
    let create = handle_admin_pair_create(
        &state,
        &p,
        &underwrap_protocol::service::AdminPairCreateRequest {
            store: store_id.clone(),
            wrapper: "gh".into(),
            resource_id: resource_id.clone(),
        },
    );
    assert_eq!(create.status(), StatusCode::OK);

    let dup = handle_admin_pair_create(
        &state,
        &p,
        &underwrap_protocol::service::AdminPairCreateRequest {
            store: store_id.clone(),
            wrapper: "gh".into(),
            resource_id: resource_id.clone(),
        },
    );
    assert_eq!(dup.status(), StatusCode::CONFLICT);
    assert_eq!(response_body_json(dup)["code"], "pair_already_exists");
}

#[test]
fn admin_pair_delete_refuses_when_permissions_reference_the_pair() {
    use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal(&[]);

    let resource_id = create_resource_id_for_test(&state, &p, &store_id, "r");
    handle_admin_pair_create(
        &state,
        &p,
        &underwrap_protocol::service::AdminPairCreateRequest {
            store: store_id.clone(),
            wrapper: "gh".into(),
            resource_id: resource_id.clone(),
        },
    );
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

    let del = handle_admin_pair_delete(
        &state,
        &p,
        &underwrap_protocol::service::AdminPairDeleteRequest {
            store: store_id.clone(),
            wrapper: "gh".into(),
            resource_id: resource_id.clone(),
        },
    );
    assert_eq!(del.status(), StatusCode::OK);
    let body = response_body_json(del);
    assert_eq!(body["ok"], false);
    assert_eq!(body["blocked_by"]["permission_count"], 1);
    assert_eq!(body["blocked_by"]["sample_permissions"][0]["wrapper"], "gh");
}
