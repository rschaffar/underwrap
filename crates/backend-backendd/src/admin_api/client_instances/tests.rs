use super::*;
use crate::admin_api::policy::handle_admin_wrapper_permission_upsert;
use crate::admin_api::stores::handle_admin_store_delete;
use crate::storage::documents::load_policy_document_or_default;
use crate::test_support::{
    SAMPLE_CLIENT_CERT_PEM, admin_principal, create_store_for_test, crud_state, response_body_json,
};
use underwrap_protocol::service::{
    AdminClientInstanceCreateRequest, AdminClientInstanceStatus, AdminClientInstanceUpdateRequest,
    AdminStoreBackendKind, AdminStoreCreateRequest,
};

#[test]
fn client_instance_crud_and_store_mapping_round_trip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let p = admin_principal(&[]);
    let work_id = create_store_for_test(
        &state,
        &p,
        AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    let other_id = create_store_for_test(
        &state,
        &p,
        AdminStoreCreateRequest {
            name: "other".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );

    let create = do_admin_client_instance_create(
        &state,
        &p,
        AdminClientInstanceCreateRequest {
            display_name: "laptop-work".into(),
            current_store_id: work_id.clone(),
            status: AdminClientInstanceStatus::Active,
            cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
            issued_at: Some(111),
            auto_create_wrapper_assets: None,
        },
    );
    assert_eq!(create.status(), StatusCode::OK);
    let created = response_body_json(create);
    let client_id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["current_store_id"], work_id);
    assert_eq!(created["current_store_name"], "work");
    assert_eq!(created["actions"]["can_edit_display_name"], true);
    assert_eq!(created["actions"]["can_remap_store"], true);
    assert_eq!(created["actions"]["can_disable"], true);
    assert_eq!(created["actions"]["can_enable"], false);
    assert_eq!(created["actions"]["can_delete"], true);

    let list = handle_admin_client_instances_list(&state);
    let list_body = response_body_json(list);
    assert_eq!(list_body["items"].as_array().unwrap().len(), 1);
    assert_eq!(list_body["items"][0]["id"], client_id);
    assert_eq!(list_body["items"][0]["current_store_name"], "work");
    assert_eq!(list_body["items"][0]["actions"]["can_disable"], true);

    let delete_in_use = handle_admin_store_delete(&state, &p, &work_id, true);
    assert_eq!(delete_in_use.status(), StatusCode::CONFLICT);
    assert_eq!(response_body_json(delete_in_use)["code"], "store_in_use");

    let update = do_admin_client_instance_update(
        &state,
        &p,
        &client_id,
        AdminClientInstanceUpdateRequest {
            display_name: Some("laptop-renamed".into()),
            current_store_id: Some(other_id.clone()),
            status: Some(AdminClientInstanceStatus::Disabled),
            cert_pem: None,
            auto_create_wrapper_assets: None,
        },
    );
    assert_eq!(update.status(), StatusCode::OK);
    let updated = response_body_json(update);
    assert_eq!(updated["display_name"], "laptop-renamed");
    assert_eq!(updated["current_store_id"], other_id);
    assert_eq!(updated["current_store_name"], "other");
    assert_eq!(updated["status"], "disabled");
    assert_eq!(updated["actions"]["can_disable"], false);
    assert_eq!(updated["actions"]["can_enable"], true);

    let list_after_disable = response_body_json(handle_admin_client_instances_list(&state));
    assert_eq!(list_after_disable["items"].as_array().unwrap().len(), 1);
    assert_eq!(list_after_disable["items"][0]["id"], client_id);
    assert_eq!(list_after_disable["items"][0]["status"], "disabled");

    let delete_old_store = handle_admin_store_delete(&state, &p, &work_id, false);
    assert_eq!(delete_old_store.status(), StatusCode::OK);

    let delete_client = handle_admin_client_instance_delete(&state, &p, &client_id);
    assert_eq!(delete_client.status(), StatusCode::OK);
    assert_eq!(response_body_json(delete_client)["ok"], true);
}

#[test]
fn client_instance_delete_cascades_to_wrapper_permissions() {
    use underwrap_protocol::policy::{WrapperApprovalMode, WrapperPermission};
    use underwrap_protocol::service::AdminWrapperPermissionUpsertRequest;

    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let p = admin_principal(&[]);
    let store_id = create_store_for_test(
        &state,
        &p,
        AdminStoreCreateRequest {
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
        let upsert = handle_admin_wrapper_permission_upsert(
            &state,
            &p,
            &AdminWrapperPermissionUpsertRequest {
                store: store_id.clone(),
                permission: WrapperPermission {
                    client_instance: instance.clone(),
                    wrapper: "gh".into(),
                    resource_id: "github/token".into(),
                    mode: WrapperApprovalMode::Unlocked,
                    writable: false,
                    reason: None,
                },
            },
        );
        assert_eq!(upsert.status(), StatusCode::OK);
    }

    let before = load_policy_document_or_default(
        state
            .stores
            .read()
            .unwrap()
            .expect_unlocked(&store_id)
            .as_ref(),
    )
    .unwrap();
    assert_eq!(before.permissions.len(), 2);

    let delete = handle_admin_client_instance_delete(&state, &p, &doomed_id);
    assert_eq!(delete.status(), StatusCode::OK);

    // The doomed instance's permission is gone; the survivor's
    // remains so we know we cascaded by id, not by store-wide nuke.
    let after = load_policy_document_or_default(
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
fn client_instance_auto_create_flag_defaults_off_and_can_be_toggled() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let p = admin_principal(&[]);
    let store_id = create_store_for_test(
        &state,
        &p,
        AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );

    // Default: when the create request omits the flag the stored
    // value is false and the detail/list response reports it.
    let create = do_admin_client_instance_create(
        &state,
        &p,
        AdminClientInstanceCreateRequest {
            display_name: "default-flag".into(),
            current_store_id: store_id.clone(),
            status: AdminClientInstanceStatus::Active,
            cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
            issued_at: None,
            auto_create_wrapper_assets: None,
        },
    );
    assert_eq!(create.status(), StatusCode::OK);
    let created = response_body_json(create);
    assert_eq!(created["auto_create_wrapper_assets"], false);
    let id = created["id"].as_str().unwrap().to_string();

    // Update toggles it on.
    let toggled = do_admin_client_instance_update(
        &state,
        &p,
        &id,
        AdminClientInstanceUpdateRequest {
            display_name: None,
            current_store_id: None,
            status: None,
            cert_pem: None,
            auto_create_wrapper_assets: Some(true),
        },
    );
    assert_eq!(toggled.status(), StatusCode::OK);
    let toggled_body = response_body_json(toggled);
    assert_eq!(toggled_body["auto_create_wrapper_assets"], true);

    // Persisted across a fresh load.
    let reloaded = crate::runtime_clients::load_client_instances(&state.client_instances_file)
        .expect("reload client instances");
    assert!(reloaded.get(&id).unwrap().auto_create_wrapper_assets);

    // Update without the flag leaves it alone.
    let kept = do_admin_client_instance_update(
        &state,
        &p,
        &id,
        AdminClientInstanceUpdateRequest {
            display_name: Some("renamed".into()),
            current_store_id: None,
            status: None,
            cert_pem: None,
            auto_create_wrapper_assets: None,
        },
    );
    assert_eq!(kept.status(), StatusCode::OK);
    assert_eq!(response_body_json(kept)["auto_create_wrapper_assets"], true);

    // And it can be toggled back off explicitly.
    let off = do_admin_client_instance_update(
        &state,
        &p,
        &id,
        AdminClientInstanceUpdateRequest {
            display_name: None,
            current_store_id: None,
            status: None,
            cert_pem: None,
            auto_create_wrapper_assets: Some(false),
        },
    );
    assert_eq!(off.status(), StatusCode::OK);
    assert_eq!(response_body_json(off)["auto_create_wrapper_assets"], false);
}
