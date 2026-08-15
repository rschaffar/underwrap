use std::collections::HashMap;
use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, State};
use hyper::StatusCode;
use underwrap_backend_store::StorageBackend;
use underwrap_protocol::entities::{
    EntityCreator, PairsDocument, ResourceRecord, ResourcesDocument, WrapperResourcePairRecord,
};
use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};

use super::*;
use crate::http::responses::Body;
use crate::runtime_clients::{ClientInstanceRecord, ClientInstanceStatus, CurrentCredentialRecord};
use crate::storage::documents::{
    load_policy_document, save_pairs_document, save_policy_document, save_resources_document,
};
use crate::test_support::{
    TestStore, admin_principal, capture_tracing_output, crud_state, run_handler, test_admin_state,
};

fn capture(run: impl FnOnce()) -> String {
    capture_tracing_output(run).1
}

#[test]
fn drop_client_instance_permissions_on_locked_store_warns_and_audits() {
    use underwrap_protocol::service::{AdminStoreBackendKind, AdminStoreCreateRequest};
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let store_id = crate::test_support::create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );

    // Force the registry into the Locked state for this store id.
    state.shared.stores.write().unwrap().lock(&store_id);

    let out = capture(|| {
        drop_client_instance_permissions(&state, &p, &store_id, "inst_xyz")
            .expect("locked store must not propagate Err");
    });

    // Both the structured tracing::warn! and the
    // log_admin_event line must fire so operators can find the
    // orphan rows.
    assert!(
        out.contains("store locked"),
        "expected the warn! line, got: {out}",
    );
    assert!(
        out.contains("action=client-instance.orphan-permissions"),
        "expected the audit event, got: {out}",
    );
    assert!(
        out.contains("resource=Some(\"client-instance:inst_xyz\")"),
        "audit must name the orphaned client instance, got: {out}",
    );
    assert!(
        out.contains("operation=Some(\"store-locked\")"),
        "audit must surface the store-locked reason, got: {out}",
    );
    assert!(
        out.contains("result=skipped"),
        "audit must report result=skipped, got: {out}",
    );
}

#[test]
fn drop_client_instance_permissions_on_missing_store_is_a_quiet_noop() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();

    let out = capture(|| {
        drop_client_instance_permissions(&state, &p, "store-that-never-existed", "inst_xyz")
            .expect("missing store must not propagate Err");
    });

    // Missing-store cascade is info-level only; no audit line.
    assert!(
        !out.contains("action=client-instance.orphan-permissions"),
        "missing-store path must not emit the orphan audit event, got: {out}",
    );
}

fn upsert(
    state: Arc<ListenerState>,
    store_id: &str,
    client_instance: &str,
    wrapper: &str,
    resource_id: &str,
    body: AdminWrapperPermissionUpsertRequest,
) -> (StatusCode, serde_json::Value) {
    run_handler(handle_admin_wrapper_permission_upsert(
        State(state),
        Extension(admin_principal()),
        Path((
            store_id.to_string(),
            client_instance.to_string(),
            wrapper.to_string(),
            resource_id.to_string(),
        )),
        Body(body),
    ))
}

fn assert_reference_error(result: &(StatusCode, serde_json::Value), expected_message: &str) {
    assert_eq!(result.0, StatusCode::NOT_FOUND, "{}", result.1);
    assert_eq!(result.1["code"], "not_found");
    assert_eq!(result.1["message"], expected_message);
}

struct PermissionFixture {
    state: Arc<ListenerState>,
    store: Arc<TestStore>,
    store_id: String,
    instance_id: String,
    resource_id: String,
    wrapper: String,
}

impl PermissionFixture {
    fn new(status: ClientInstanceStatus, policy: PolicyDocument) -> Self {
        let store = Arc::new(TestStore::new());
        let store_id = "work".to_string();
        let instance_id = "inst-published".to_string();
        let resource_id = "res-exact".to_string();
        let wrapper = "gh-exact".to_string();
        let state = test_admin_state(HashMap::from([(
            store_id.clone(),
            store.clone() as Arc<dyn StorageBackend>,
        )]));
        state.shared.client_instances.write().unwrap().insert(
            instance_id.clone(),
            ClientInstanceRecord {
                id: instance_id.clone(),
                display_name: "Published client".into(),
                current_store_id: store_id.clone(),
                status,
                last_seen_at: None,
                current_credential: CurrentCredentialRecord {
                    cert_fingerprint: "test-fingerprint".into(),
                    cert_pem: "test-certificate".into(),
                },
                auto_create_wrapper_assets: false,
            },
        );
        let mut resources = ResourcesDocument::default();
        resources
            .insert(ResourceRecord {
                id: resource_id.clone(),
                name: "Exact resource".into(),
                created_by: EntityCreator::Operator {
                    id: "admin-id".into(),
                    username: "alice".into(),
                },
                created_at: 1,
            })
            .unwrap();
        save_resources_document(store.as_ref(), &resources).unwrap();
        let mut pairs = PairsDocument::default();
        pairs
            .insert(WrapperResourcePairRecord {
                wrapper: wrapper.clone(),
                resource_id: resource_id.clone(),
                created_by: EntityCreator::ClientInstance {
                    id: instance_id.clone(),
                },
                created_at: 1,
                last_used_at: None,
            })
            .unwrap();
        save_pairs_document(store.as_ref(), &pairs).unwrap();
        save_policy_document(store.as_ref(), &policy).unwrap();
        store.clear_operation_trace();
        Self {
            state: Arc::new(state),
            store,
            store_id,
            instance_id,
            resource_id,
            wrapper,
        }
    }

    fn policy_bytes(&self) -> Vec<u8> {
        self.store.data.lock().unwrap()["control/policy"].clone()
    }

    fn assert_rejection_unchanged(
        &self,
        result: &(StatusCode, serde_json::Value),
        expected_message: &str,
        before: &[u8],
        expected_trace: &[&str],
    ) {
        assert_reference_error(result, expected_message);
        assert_eq!(self.policy_bytes(), before);
        assert_eq!(
            self.store.operation_trace(),
            expected_trace
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );
    }
}

fn stale_permission(client: &str, wrapper: &str, resource: &str) -> WrapperPermission {
    WrapperPermission {
        client_instance: client.into(),
        wrapper: wrapper.into(),
        resource_id: resource.into(),
        mode: WrapperApprovalMode::Locked,
        writable: false,
        reason: Some("stale row".into()),
    }
}

#[test]
fn admin_reference_permission_rejections_follow_order_and_preserve_raw_policy_bytes() {
    let fixture = PermissionFixture::new(ClientInstanceStatus::Active, PolicyDocument::default());

    let before = fixture.policy_bytes();
    fixture.store.clear_operation_trace();
    let missing_client = upsert(
        fixture.state.clone(),
        &fixture.store_id,
        "missing-client",
        "missing-wrapper",
        "missing-resource",
        AdminWrapperPermissionUpsertRequest {
            mode: WrapperApprovalMode::Unlocked,
            writable: false,
            reason: None,
        },
    );
    fixture.assert_rejection_unchanged(&missing_client, "unknown client instance", &before, &[]);

    fixture
        .state
        .shared
        .client_instances
        .write()
        .unwrap()
        .get_mut(&fixture.instance_id)
        .unwrap()
        .current_store_id = "other-store".into();
    let before = fixture.policy_bytes();
    fixture.store.clear_operation_trace();
    let wrong_store = upsert(
        fixture.state.clone(),
        &fixture.store_id,
        &fixture.instance_id,
        "missing-wrapper",
        "missing-resource",
        AdminWrapperPermissionUpsertRequest {
            mode: WrapperApprovalMode::Unlocked,
            writable: false,
            reason: None,
        },
    );
    fixture.assert_rejection_unchanged(&wrong_store, "unknown client instance", &before, &[]);
    fixture
        .state
        .shared
        .client_instances
        .write()
        .unwrap()
        .get_mut(&fixture.instance_id)
        .unwrap()
        .current_store_id = fixture.store_id.clone();

    let before = fixture.policy_bytes();
    fixture.store.clear_operation_trace();
    let missing_resource = upsert(
        fixture.state.clone(),
        &fixture.store_id,
        &fixture.instance_id,
        "missing-wrapper",
        "missing-resource",
        AdminWrapperPermissionUpsertRequest {
            mode: WrapperApprovalMode::Unlocked,
            writable: false,
            reason: None,
        },
    );
    fixture.assert_rejection_unchanged(
        &missing_resource,
        "unknown resource",
        &before,
        &["read:control/resources"],
    );

    let before = fixture.policy_bytes();
    fixture.store.clear_operation_trace();
    let missing_pair = upsert(
        fixture.state.clone(),
        &fixture.store_id,
        &fixture.instance_id,
        "missing-wrapper",
        &fixture.resource_id,
        AdminWrapperPermissionUpsertRequest {
            mode: WrapperApprovalMode::Unlocked,
            writable: false,
            reason: None,
        },
    );
    fixture.assert_rejection_unchanged(
        &missing_pair,
        "unknown pair",
        &before,
        &["read:control/resources", "read:control/pairs"],
    );
}

#[test]
fn admin_reference_permission_accepts_disabled_client_and_sorts_create_and_update() {
    let existing = stale_permission("zz-existing", "wrapper", "resource");
    let fixture = PermissionFixture::new(
        ClientInstanceStatus::Disabled,
        PolicyDocument {
            permissions: vec![existing.clone()],
        },
    );

    let (created, create_audit) = capture_tracing_output(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(handle_admin_wrapper_permission_upsert(
                State(fixture.state.clone()),
                Extension(admin_principal()),
                Path((
                    fixture.store_id.clone(),
                    fixture.instance_id.clone(),
                    fixture.wrapper.clone(),
                    fixture.resource_id.clone(),
                )),
                Body(AdminWrapperPermissionUpsertRequest {
                    mode: WrapperApprovalMode::Locked,
                    writable: false,
                    reason: Some("initial reason must not be logged".into()),
                }),
            ))
    });
    assert!(created.is_ok());
    assert!(create_audit.contains("action=wrapper_permission.upsert"));
    assert!(create_audit.contains("result=success"));
    assert!(!create_audit.contains("initial reason must not be logged"));
    let policy = load_policy_document(fixture.store.as_ref()).unwrap();
    assert_eq!(policy.permissions.len(), 2);
    assert_eq!(policy.permissions[0].client_instance, fixture.instance_id);
    assert_eq!(policy.permissions[1], existing);
    assert!(matches!(
        policy.permissions[0].mode,
        WrapperApprovalMode::Locked
    ));
    assert!(!policy.permissions[0].writable);
    assert_eq!(
        policy.permissions[0].reason.as_deref(),
        Some("initial reason must not be logged")
    );

    let (updated, update_audit) = capture_tracing_output(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(handle_admin_wrapper_permission_upsert(
                State(fixture.state.clone()),
                Extension(admin_principal()),
                Path((
                    fixture.store_id.clone(),
                    fixture.instance_id.clone(),
                    fixture.wrapper.clone(),
                    fixture.resource_id.clone(),
                )),
                Body(AdminWrapperPermissionUpsertRequest {
                    mode: WrapperApprovalMode::Unlocked,
                    writable: true,
                    reason: None,
                }),
            ))
    });
    assert!(updated.is_ok());
    assert!(update_audit.contains("action=wrapper_permission.upsert"));
    assert!(update_audit.contains("result=success"));
    assert!(!update_audit.contains("initial reason must not be logged"));
    let policy = load_policy_document(fixture.store.as_ref()).unwrap();
    assert_eq!(policy.permissions.len(), 2);
    assert_eq!(policy.permissions[0].client_instance, fixture.instance_id);
    assert_eq!(policy.permissions[1], existing);
    assert!(matches!(
        policy.permissions[0].mode,
        WrapperApprovalMode::Unlocked
    ));
    assert!(policy.permissions[0].writable);
    assert_eq!(policy.permissions[0].reason, None);
}

#[test]
fn stale_wrapper_permission_remains_listable_and_deletable_without_live_references() {
    let stale = stale_permission("removed-client", "removed-wrapper", "removed-resource");
    let fixture = PermissionFixture::new(
        ClientInstanceStatus::Active,
        PolicyDocument {
            permissions: vec![stale.clone()],
        },
    );

    fixture.store.clear_operation_trace();
    let (status, body) = run_handler(handle_admin_wrapper_permissions_list(
        State(fixture.state.clone()),
        Path(fixture.store_id.clone()),
    ));
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["permissions"], serde_json::to_value([&stale]).unwrap());
    assert_eq!(fixture.store.operation_trace(), ["read:control/policy"]);

    fixture.store.clear_operation_trace();
    let (status, body) = run_handler(handle_admin_wrapper_permission_delete(
        State(fixture.state),
        Extension(admin_principal()),
        Path((
            fixture.store_id,
            stale.client_instance,
            stale.wrapper,
            stale.resource_id,
        )),
    ));
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true);
    assert_eq!(
        fixture.store.operation_trace(),
        ["read:control/policy", "write:control/policy"]
    );
    assert!(
        load_policy_document(fixture.store.as_ref())
            .unwrap()
            .permissions
            .is_empty()
    );
}
