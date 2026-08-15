use axum::Extension;
use axum::extract::{Path, State};
use hyper::StatusCode;

use super::*;
use crate::http::responses::Body;
use crate::storage::documents::{load_pairs_document, save_policy_document};
use crate::test_support::{
    admin_principal, create_resource_id_for_test, entity_test_state_with_store, run_handler,
};

#[test]
fn admin_reference_pair_create_requires_existing_resource_and_refuses_duplicates() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();

    let store = state
        .shared
        .stores
        .read()
        .unwrap()
        .expect_unlocked(&store_id);
    let pairs_before = load_pairs_document(store.as_ref()).unwrap();

    // No resource -> canonical value-free 404 and no pairs mutation.
    let (status, body) = run_handler(handle_admin_pair_create(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
        Body(underwrap_protocol::service::AdminPairCreateRequest {
            wrapper: "gh".into(),
            resource_id: "res_missing".into(),
        }),
    ));
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
    assert_eq!(body["message"], "unknown resource");
    assert!(!body.to_string().contains("res_missing"));
    assert_eq!(load_pairs_document(store.as_ref()).unwrap(), pairs_before);
    drop(store);

    let resource_id = create_resource_id_for_test(state.clone(), &p, &store_id, "github/token");
    let (create_status, _) = run_handler(handle_admin_pair_create(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
        Body(underwrap_protocol::service::AdminPairCreateRequest {
            wrapper: "gh".into(),
            resource_id: resource_id.clone(),
        }),
    ));
    assert_eq!(create_status, StatusCode::OK);

    let (dup_status, dup_body) = run_handler(handle_admin_pair_create(
        State(state.clone()),
        Extension(p),
        Path(store_id),
        Body(underwrap_protocol::service::AdminPairCreateRequest {
            wrapper: "gh".into(),
            resource_id,
        }),
    ));
    assert_eq!(dup_status, StatusCode::CONFLICT);
    assert_eq!(dup_body["code"], "pair_already_exists");
}

#[test]
fn admin_pair_delete_refuses_when_permissions_reference_the_pair() {
    use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();

    let resource_id = create_resource_id_for_test(state.clone(), &p, &store_id, "r");
    let (create_status, _) = run_handler(handle_admin_pair_create(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
        Body(underwrap_protocol::service::AdminPairCreateRequest {
            wrapper: "gh".into(),
            resource_id: resource_id.clone(),
        }),
    ));
    assert_eq!(create_status, StatusCode::OK);
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

    let (status, body) = run_handler(handle_admin_pair_delete(
        State(state.clone()),
        Extension(p),
        Path((store_id, "gh".into(), resource_id)),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], false);
    assert_eq!(body["blocked_by"]["permission_count"], 1);
    assert_eq!(body["blocked_by"]["sample_permissions"][0]["wrapper"], "gh");
}

#[test]
fn admin_pair_with_new_resource_creates_both_in_one_request() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();

    let (status, body) = run_handler(handle_admin_pair_with_new_resource(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
        Body(
            underwrap_protocol::service::AdminPairWithNewResourceRequest {
                wrapper: "gh".into(),
                resource_name: "github/token".into(),
            },
        ),
    ));
    assert_eq!(status, StatusCode::OK);
    let resource_id = body["resource"]["id"].as_str().unwrap().to_string();
    assert_eq!(body["resource"]["name"], "github/token");
    assert_eq!(body["pair"]["wrapper"], "gh");
    assert_eq!(body["pair"]["resource_id"], resource_id);
}

#[test]
fn admin_pair_with_new_resource_rejects_duplicate_name() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();

    let _ = create_resource_id_for_test(state.clone(), &p, &store_id, "github/token");
    let (status, body) = run_handler(handle_admin_pair_with_new_resource(
        State(state.clone()),
        Extension(p),
        Path(store_id),
        Body(
            underwrap_protocol::service::AdminPairWithNewResourceRequest {
                wrapper: "gh".into(),
                resource_name: "github/token".into(),
            },
        ),
    ));
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "resource_already_exists");
}

#[test]
fn admin_pair_with_new_resource_validates_inputs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let p = admin_principal();

    let (status_w, body_w) = run_handler(handle_admin_pair_with_new_resource(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
        Body(
            underwrap_protocol::service::AdminPairWithNewResourceRequest {
                wrapper: "  ".into(),
                resource_name: "github/token".into(),
            },
        ),
    ));
    assert_eq!(status_w, StatusCode::BAD_REQUEST);
    assert_eq!(body_w["code"], "invalid_pair");

    let (status_r, body_r) = run_handler(handle_admin_pair_with_new_resource(
        State(state),
        Extension(p),
        Path(store_id),
        Body(
            underwrap_protocol::service::AdminPairWithNewResourceRequest {
                wrapper: "gh".into(),
                resource_name: "  ".into(),
            },
        ),
    ));
    assert_eq!(status_r, StatusCode::BAD_REQUEST);
    assert_eq!(body_r["code"], "invalid_resource_name");
}
