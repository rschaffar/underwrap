use axum::response::IntoResponse;
use hyper::StatusCode;

use super::*;
use crate::runtime_clients::{ClientInstanceStatus, CurrentCredentialRecord};
use crate::storage::documents::{load_pairs_document, load_resources_document};
use crate::test_support::make_runtime_api_fixture;

fn assert_not_found(error: AdminError, message: &str) {
    let response = error.into_response();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let (_, body) = response.into_parts();
    let bytes = crate::test_support::block_on(axum::body::to_bytes(body, 1024)).unwrap();
    let envelope: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(envelope["code"], "not_found");
    assert_eq!(envelope["message"], message);
    assert!(!envelope.to_string().contains("missing"));
}

#[test]
fn admin_reference_resource_is_exact_and_value_free() {
    let fixture = make_runtime_api_fixture();
    let store = fixture.store();
    require_resource(store.as_ref(), &fixture.resource_id).unwrap();
    assert_not_found(
        require_resource(store.as_ref(), "missing-resource").unwrap_err(),
        "unknown resource",
    );
    assert_eq!(
        load_resources_document(store.as_ref())
            .unwrap()
            .resources
            .len(),
        1
    );
}

#[test]
fn admin_reference_client_requires_selected_store_but_accepts_disabled() {
    let fixture = make_runtime_api_fixture();
    require_client_instance_in_store(&fixture.state, &fixture.store_id, &fixture.instance_id)
        .unwrap();
    fixture
        .state
        .shared
        .client_instances
        .write()
        .unwrap()
        .get_mut(&fixture.instance_id)
        .unwrap()
        .status = ClientInstanceStatus::Disabled;
    require_client_instance_in_store(&fixture.state, &fixture.store_id, &fixture.instance_id)
        .unwrap();

    assert_not_found(
        require_client_instance_in_store(&fixture.state, "other-store", &fixture.instance_id)
            .unwrap_err(),
        "unknown client instance",
    );
    assert_not_found(
        require_client_instance_in_store(&fixture.state, &fixture.store_id, "missing-client")
            .unwrap_err(),
        "unknown client instance",
    );

    // Keep the fixture record structurally realistic after the direct status
    // mutation above; reference validity intentionally ignores credential data.
    let record = fixture
        .state
        .shared
        .client_instances
        .read()
        .unwrap()
        .get(&fixture.instance_id)
        .unwrap()
        .clone();
    assert_eq!(
        record.current_credential,
        CurrentCredentialRecord {
            cert_fingerprint: "fp-runtime".into(),
            cert_pem: crate::test_support::SAMPLE_CLIENT_CERT_PEM.into(),
        }
    );
}

#[test]
fn admin_reference_pair_is_exact_and_value_free() {
    let fixture = make_runtime_api_fixture();
    let store = fixture.store();
    require_pair(store.as_ref(), "gh", &fixture.resource_id).unwrap();
    assert_not_found(
        require_pair(store.as_ref(), "other-wrapper", &fixture.resource_id).unwrap_err(),
        "unknown pair",
    );
    assert_eq!(load_pairs_document(store.as_ref()).unwrap().pairs.len(), 1);
}
