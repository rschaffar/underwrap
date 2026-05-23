use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, Query, State};
use hyper::StatusCode;
use underwrap_protocol::service::{
    AdminClientInstanceCreateRequest, AdminClientInstanceStatus, AdminClientInstanceUpdateRequest,
    AdminStoreBackendKind, AdminStoreCreateRequest,
};

use super::*;
use crate::admin_api::policy::handle_admin_wrapper_permission_upsert;
use crate::admin_api::stores::crud::{DeleteQuery, handle_admin_store_delete};
use crate::http::responses::Body;
use crate::storage::documents::load_policy_document;
use crate::test_support::{
    SAMPLE_CLIENT_CERT_PEM, admin_principal, create_store_for_test, crud_state, run_handler,
};

fn delete_store(
    state: Arc<crate::runtime_state::AdminRuntimeState>,
    p: &crate::users::Principal,
    name: &str,
    force: bool,
) -> (StatusCode, serde_json::Value) {
    let q = DeleteQuery {
        force: force.then(|| "true".to_string()),
    };
    run_handler(handle_admin_store_delete(
        State(state),
        Extension(p.clone()),
        Path(name.to_string()),
        Query(q),
    ))
}

#[test]
fn client_instance_crud_and_store_mapping_round_trip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let work_id = create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    let other_id = create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "other".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );

    let (status, created) = run_handler(handle_admin_client_instance_create(
        State(state.clone()),
        Extension(p.clone()),
        Body(AdminClientInstanceCreateRequest {
            display_name: "laptop-work".into(),
            current_store_id: work_id.clone(),
            status: AdminClientInstanceStatus::Active,
            cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
            issued_at: Some(111),
            auto_create_wrapper_assets: None,
        }),
    ));
    assert_eq!(status, StatusCode::OK);
    let client_id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["current_store_id"], work_id);
    assert_eq!(created["current_store_name"], "work");
    assert_eq!(created["actions"]["can_disable"], true);
    assert_eq!(created["actions"]["can_enable"], false);

    let (list_status, list_body) =
        run_handler(handle_admin_client_instances_list(State(state.clone())));
    assert_eq!(list_status, StatusCode::OK);
    assert_eq!(list_body["items"].as_array().unwrap().len(), 1);
    assert_eq!(list_body["items"][0]["id"], client_id);
    assert_eq!(list_body["items"][0]["current_store_name"], "work");
    assert_eq!(list_body["items"][0]["actions"]["can_disable"], true);

    let (in_use_status, in_use_body) = delete_store(state.clone(), &p, &work_id, true);
    assert_eq!(in_use_status, StatusCode::CONFLICT);
    assert_eq!(in_use_body["code"], "store_in_use");

    let (update_status, updated) = run_handler(handle_admin_client_instance_update(
        State(state.clone()),
        Extension(p.clone()),
        Path(client_id.clone()),
        Body(AdminClientInstanceUpdateRequest {
            display_name: Some("laptop-renamed".into()),
            current_store_id: Some(other_id.clone()),
            status: Some(AdminClientInstanceStatus::Disabled),
            cert_pem: None,
            auto_create_wrapper_assets: None,
        }),
    ));
    assert_eq!(update_status, StatusCode::OK);
    assert_eq!(updated["display_name"], "laptop-renamed");
    assert_eq!(updated["current_store_id"], other_id);
    assert_eq!(updated["current_store_name"], "other");
    assert_eq!(updated["status"], "disabled");
    assert_eq!(updated["actions"]["can_disable"], false);
    assert_eq!(updated["actions"]["can_enable"], true);

    let (_, list_after_disable) =
        run_handler(handle_admin_client_instances_list(State(state.clone())));
    assert_eq!(list_after_disable["items"].as_array().unwrap().len(), 1);
    assert_eq!(list_after_disable["items"][0]["id"], client_id);
    assert_eq!(list_after_disable["items"][0]["status"], "disabled");

    let (delete_old_status, _) = delete_store(state.clone(), &p, &work_id, false);
    assert_eq!(delete_old_status, StatusCode::OK);

    let (delete_client_status, delete_client_body) = run_handler(
        handle_admin_client_instance_delete(State(state.clone()), Extension(p), Path(client_id)),
    );
    assert_eq!(delete_client_status, StatusCode::OK);
    assert_eq!(delete_client_body["ok"], true);
}

#[test]
fn client_instance_delete_cascades_to_wrapper_permissions() {
    use underwrap_protocol::policy::WrapperApprovalMode;
    use underwrap_protocol::service::AdminWrapperPermissionUpsertRequest;

    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let store_id = create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );

    // Two distinct client instances on the same store: only the
    // first one is deleted, so the second one's permission must
    // survive the cascade. The fingerprint uniqueness check in the
    // runtime client registry forces us to bypass the create API
    // (which validates a real PEM) and seed records directly.
    let doomed_id = "doomed".to_string();
    let survivor_id = "survivor".to_string();
    {
        let mut instances = state.client_instances.write().unwrap();
        for (id, fp) in [(&doomed_id, "fp-doomed"), (&survivor_id, "fp-survivor")] {
            instances.insert(
                id.clone(),
                crate::runtime_clients::ClientInstanceRecord {
                    id: id.clone(),
                    display_name: format!("{id}-laptop"),
                    current_store_id: store_id.clone(),
                    status: crate::runtime_clients::ClientInstanceStatus::Active,
                    last_seen_at: None,
                    current_credential: crate::runtime_clients::CurrentCredentialRecord {
                        cert_fingerprint: fp.into(),
                        cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
                        issued_at: Some(1),
                    },
                    auto_create_wrapper_assets: false,
                },
            );
        }
        crate::runtime_clients::save_client_instances(&state.client_instances_file, &instances)
            .unwrap();
    }

    // The admin API addresses stores by id (the SPA's `storeName`
    // state is misleadingly named — it carries the store id).
    for instance in [&doomed_id, &survivor_id] {
        let (status, _) = run_handler(handle_admin_wrapper_permission_upsert(
            State(state.clone()),
            Extension(p.clone()),
            axum::extract::Path((
                store_id.clone(),
                instance.clone(),
                "gh".into(),
                "github/token".into(),
            )),
            Body(AdminWrapperPermissionUpsertRequest {
                mode: WrapperApprovalMode::Unlocked,
                writable: false,
                reason: None,
            }),
        ));
        assert_eq!(status, StatusCode::OK);
    }

    let before = load_policy_document(
        state
            .stores
            .read()
            .unwrap()
            .expect_unlocked(&store_id)
            .as_ref(),
    )
    .unwrap();
    assert_eq!(before.permissions.len(), 2);

    let (delete_status, _) = run_handler(handle_admin_client_instance_delete(
        State(state.clone()),
        Extension(p.clone()),
        Path(doomed_id),
    ));
    assert_eq!(delete_status, StatusCode::OK);

    // The doomed instance's permission is gone; the survivor's
    // remains so we know we cascaded by id, not by store-wide nuke.
    let after = load_policy_document(
        state
            .stores
            .read()
            .unwrap()
            .expect_unlocked(&store_id)
            .as_ref(),
    )
    .unwrap();
    assert_eq!(
        after.permissions.len(),
        1,
        "only the survivor's permission should remain: {after:?}",
    );
    assert_eq!(after.permissions[0].client_instance, survivor_id);
}

#[test]
fn client_instance_update_returns_ok_when_remap_cascade_fails() {
    // Regression for the post-commit ordering bug: the on-disk
    // registry update lands before the cascade against the old
    // store's policy document. If the cascade fails the response
    // must still report success — returning 500 would leave the
    // caller assuming the remap did not take while the registry
    // already holds the new mapping.
    use underwrap_protocol::policy::POLICY_DOCUMENT_SECRET_ID;
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let old_store_id = create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "old".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    let new_store_id = create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "new".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    let instance_id = "to-remap".to_string();
    {
        let mut instances = state.client_instances.write().unwrap();
        instances.insert(
            instance_id.clone(),
            crate::runtime_clients::ClientInstanceRecord {
                id: instance_id.clone(),
                display_name: "laptop".into(),
                current_store_id: old_store_id.clone(),
                status: crate::runtime_clients::ClientInstanceStatus::Active,
                last_seen_at: None,
                current_credential: crate::runtime_clients::CurrentCredentialRecord {
                    cert_fingerprint: "fp-remap".into(),
                    cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
                    issued_at: Some(1),
                },
                auto_create_wrapper_assets: false,
            },
        );
        crate::runtime_clients::save_client_instances(&state.client_instances_file, &instances)
            .unwrap();
    }

    // Corrupt the old store's policy document so the cascade's
    // load_policy_document() call returns Err.
    {
        let old_backend = state.stores.read().unwrap().expect_unlocked(&old_store_id);
        old_backend
            .write(POLICY_DOCUMENT_SECRET_ID, b"{ not json")
            .unwrap();
    }

    let (status, _) = run_handler(handle_admin_client_instance_update(
        State(state.clone()),
        Extension(p),
        Path(instance_id.clone()),
        Body(AdminClientInstanceUpdateRequest {
            display_name: None,
            current_store_id: Some(new_store_id.clone()),
            status: None,
            cert_pem: None,
            auto_create_wrapper_assets: None,
        }),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        state
            .client_instances
            .read()
            .unwrap()
            .get(&instance_id)
            .unwrap()
            .current_store_id,
        new_store_id,
    );
}

#[test]
fn client_instance_auto_create_flag_defaults_off_and_can_be_toggled() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let store_id = create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );

    // Default: when the create request omits the flag the stored
    // value is false and the detail/list response reports it.
    let (status, created) = run_handler(handle_admin_client_instance_create(
        State(state.clone()),
        Extension(p.clone()),
        Body(AdminClientInstanceCreateRequest {
            display_name: "default-flag".into(),
            current_store_id: store_id.clone(),
            status: AdminClientInstanceStatus::Active,
            cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
            issued_at: None,
            auto_create_wrapper_assets: None,
        }),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["auto_create_wrapper_assets"], false);
    let id = created["id"].as_str().unwrap().to_string();

    // Update toggles it on.
    let (toggled_status, toggled_body) = run_handler(handle_admin_client_instance_update(
        State(state.clone()),
        Extension(p.clone()),
        Path(id.clone()),
        Body(AdminClientInstanceUpdateRequest {
            display_name: None,
            current_store_id: None,
            status: None,
            cert_pem: None,
            auto_create_wrapper_assets: Some(true),
        }),
    ));
    assert_eq!(toggled_status, StatusCode::OK);
    assert_eq!(toggled_body["auto_create_wrapper_assets"], true);

    // Persisted across a fresh load.
    let reloaded = crate::runtime_clients::load_client_instances(&state.client_instances_file)
        .expect("reload client instances");
    assert!(reloaded.get(&id).unwrap().auto_create_wrapper_assets);

    // Update without the flag leaves it alone.
    let (kept_status, kept_body) = run_handler(handle_admin_client_instance_update(
        State(state.clone()),
        Extension(p.clone()),
        Path(id.clone()),
        Body(AdminClientInstanceUpdateRequest {
            display_name: Some("renamed".into()),
            current_store_id: None,
            status: None,
            cert_pem: None,
            auto_create_wrapper_assets: None,
        }),
    ));
    assert_eq!(kept_status, StatusCode::OK);
    assert_eq!(kept_body["auto_create_wrapper_assets"], true);

    // And it can be toggled back off explicitly.
    let (off_status, off_body) = run_handler(handle_admin_client_instance_update(
        State(state.clone()),
        Extension(p),
        Path(id),
        Body(AdminClientInstanceUpdateRequest {
            display_name: None,
            current_store_id: None,
            status: None,
            cert_pem: None,
            auto_create_wrapper_assets: Some(false),
        }),
    ));
    assert_eq!(off_status, StatusCode::OK);
    assert_eq!(off_body["auto_create_wrapper_assets"], false);
}
