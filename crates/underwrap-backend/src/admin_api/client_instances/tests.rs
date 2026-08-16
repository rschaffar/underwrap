use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, State};
use hyper::StatusCode;
use underwrap_protocol::service::{
    AdminClientInstanceCreateRequest, AdminClientInstanceStatus, AdminClientInstanceUpdateRequest,
    AdminPairCreateRequest, AdminResourceCreateRequest, AdminStoreBackendKind,
    AdminStoreCreateRequest,
};

use super::*;
use crate::admin_api::pairs::handle_admin_pair_create;
use crate::admin_api::policy::handle_admin_wrapper_permission_upsert;
use crate::admin_api::resources::handle_admin_resource_create;
use crate::http::responses::Body;
use crate::storage::documents::load_policy_document;
use crate::test_support::{
    BlockedRegistryDir, SAMPLE_CLIENT_CERT_PEM, admin_principal, capture_tracing_output,
    create_store_for_test, crud_state, delete_store_for_test, run_handler,
};

fn f015_client_cert_pem(label: &str) -> String {
    let key = rcgen::KeyPair::generate().unwrap();
    let mut params = rcgen::CertificateParams::new(vec![label.to_string()]).unwrap();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, label);
    params.not_before = rcgen::date_time_ymd(2025, 1, 1);
    params.not_after = rcgen::date_time_ymd(2035, 1, 1);
    params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth];
    params.serial_number = Some(rcgen::SerialNumber::from(15));
    params.self_signed(&key).unwrap().pem()
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

    let (in_use_status, in_use_body) = delete_store_for_test(state.clone(), &p, &work_id, true);
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

    let (delete_old_status, _) = delete_store_for_test(state.clone(), &p, &work_id, false);
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
        let mut instances = state.shared.client_instances.write().unwrap();
        for (id, fp) in [(&doomed_id, "fp-doomed"), (&survivor_id, "fp-survivor")] {
            let record = crate::runtime_clients::ClientInstanceRecord {
                id: id.clone(),
                display_name: format!("{id}-laptop"),
                current_store_id: store_id.clone(),
                status: crate::runtime_clients::ClientInstanceStatus::Active,
                last_seen_at: None,
                current_credential: crate::runtime_clients::CurrentCredentialRecord {
                    cert_fingerprint: fp.into(),
                    cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
                },
                auto_create_wrapper_assets: false,
            };
            crate::runtime_clients::save_client_instance(&state.admin().clients_dir, &record)
                .unwrap();
            instances.insert(id.clone(), record);
        }
    }

    let (resource_status, resource) = run_handler(handle_admin_resource_create(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
        Body(AdminResourceCreateRequest {
            name: "GitHub token".into(),
        }),
    ));
    assert_eq!(resource_status, StatusCode::OK);
    let resource_id = resource["resource"]["id"].as_str().unwrap().to_string();

    let (pair_status, _) = run_handler(handle_admin_pair_create(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
        Body(AdminPairCreateRequest {
            wrapper: "gh".into(),
            resource_id: resource_id.clone(),
        }),
    ));
    assert_eq!(pair_status, StatusCode::OK);

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
                resource_id.clone(),
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
            .shared
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
            .shared
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
        let record = crate::runtime_clients::ClientInstanceRecord {
            id: instance_id.clone(),
            display_name: "laptop".into(),
            current_store_id: old_store_id.clone(),
            status: crate::runtime_clients::ClientInstanceStatus::Active,
            last_seen_at: None,
            current_credential: crate::runtime_clients::CurrentCredentialRecord {
                cert_fingerprint: "fp-remap".into(),
                cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
            },
            auto_create_wrapper_assets: false,
        };
        crate::runtime_clients::save_client_instance(&state.admin().clients_dir, &record).unwrap();
        state
            .shared
            .client_instances
            .write()
            .unwrap()
            .insert(instance_id.clone(), record);
    }

    // Corrupt the old store's policy document so the cascade's
    // load_policy_document() call returns Err.
    {
        let old_backend = state
            .shared
            .stores
            .read()
            .unwrap()
            .expect_unlocked(&old_store_id);
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
            .shared
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
    let reloaded = crate::runtime_clients::load_client_instances(&state.admin().clients_dir)
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

#[test]
fn f015_client_create_blocked_registry_publishes_no_candidate_and_retries_cleanly() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let store_id = create_store_for_test(
        state.clone(),
        &principal,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    let request = AdminClientInstanceCreateRequest {
        display_name: "candidate".into(),
        current_store_id: store_id,
        status: AdminClientInstanceStatus::Active,
        cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
        auto_create_wrapper_assets: Some(true),
    };

    let ((status, body), audit) = capture_tracing_output(|| {
        let _blocked = BlockedRegistryDir::new(&state.admin().clients_dir);
        run_handler(handle_admin_client_instance_create(
            State(state.clone()),
            Extension(principal.clone()),
            Body(request.clone()),
        ))
    });
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["code"], "write_client_instances_failed");
    assert!(
        !audit.contains("action=client-instance.create"),
        "failed create emitted a success audit: {audit}",
    );
    assert!(
        state.shared.client_instances.read().unwrap().is_empty(),
        "failed create published a live client candidate",
    );
    let reloaded = crate::runtime_clients::load_client_instances(&state.admin().clients_dir)
        .expect("reload client registry after restoring its directory");
    assert!(
        reloaded.is_empty(),
        "failed create left a restored-reload client candidate: {reloaded:?}",
    );

    let (retry_status, retry_body) = run_handler(handle_admin_client_instance_create(
        State(state.clone()),
        Extension(principal),
        Body(request),
    ));
    assert_eq!(retry_status, StatusCode::OK, "{retry_body}");
    let client_id = retry_body["id"].as_str().unwrap();
    let live = state
        .shared
        .client_instances
        .read()
        .unwrap()
        .get(client_id)
        .unwrap()
        .clone();
    let reloaded = crate::runtime_clients::load_client_instances(&state.admin().clients_dir)
        .expect("reload client registry after successful retry");
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded.get(client_id), Some(&live));
}

#[test]
fn f015_client_update_persists_before_publishing_and_cascading() {
    use underwrap_protocol::policy::WrapperApprovalMode;
    use underwrap_protocol::service::AdminWrapperPermissionUpsertRequest;

    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let old_store_id = create_store_for_test(
        state.clone(),
        &principal,
        &AdminStoreCreateRequest {
            name: "old".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    let new_store_id = create_store_for_test(
        state.clone(),
        &principal,
        &AdminStoreCreateRequest {
            name: "new".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    let (create_status, created) = run_handler(handle_admin_client_instance_create(
        State(state.clone()),
        Extension(principal.clone()),
        Body(AdminClientInstanceCreateRequest {
            display_name: "before".into(),
            current_store_id: old_store_id.clone(),
            status: AdminClientInstanceStatus::Active,
            cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
            auto_create_wrapper_assets: Some(false),
        }),
    ));
    assert_eq!(create_status, StatusCode::OK, "{created}");
    let client_id = created["id"].as_str().unwrap().to_string();
    let before = {
        let mut instances = state.shared.client_instances.write().unwrap();
        let record = instances.get_mut(&client_id).unwrap();
        record.last_seen_at = Some(1_234_567);
        crate::runtime_clients::save_client_instance(&state.admin().clients_dir, record).unwrap();
        record.clone()
    };

    let (resource_status, resource) = run_handler(handle_admin_resource_create(
        State(state.clone()),
        Extension(principal.clone()),
        Path(old_store_id.clone()),
        Body(AdminResourceCreateRequest {
            name: "F-015 resource".into(),
        }),
    ));
    assert_eq!(resource_status, StatusCode::OK, "{resource}");
    let resource_id = resource["resource"]["id"].as_str().unwrap().to_string();
    let (pair_status, pair) = run_handler(handle_admin_pair_create(
        State(state.clone()),
        Extension(principal.clone()),
        Path(old_store_id.clone()),
        Body(AdminPairCreateRequest {
            wrapper: "f015-wrapper".into(),
            resource_id: resource_id.clone(),
        }),
    ));
    assert_eq!(pair_status, StatusCode::OK, "{pair}");
    let (permission_status, permission) = run_handler(handle_admin_wrapper_permission_upsert(
        State(state.clone()),
        Extension(principal.clone()),
        axum::extract::Path((
            old_store_id.clone(),
            client_id.clone(),
            "f015-wrapper".into(),
            resource_id,
        )),
        Body(AdminWrapperPermissionUpsertRequest {
            mode: WrapperApprovalMode::Unlocked,
            writable: false,
            reason: Some("F-015 evidence".into()),
        }),
    ));
    assert_eq!(permission_status, StatusCode::OK, "{permission}");
    let old_backend = state
        .shared
        .stores
        .read()
        .unwrap()
        .expect_unlocked(&old_store_id);
    let policy_before = load_policy_document(old_backend.as_ref()).unwrap();
    assert_eq!(policy_before.permissions.len(), 1);

    let replacement_cert = f015_client_cert_pem("f015-replacement");
    let mut candidate = before.clone();
    candidate.display_name = "after".into();
    candidate.current_store_id = new_store_id.clone();
    candidate.status = crate::runtime_clients::ClientInstanceStatus::Disabled;
    candidate.current_credential =
        crate::runtime_clients::current_credential_from_pem(replacement_cert.clone()).unwrap();
    candidate.auto_create_wrapper_assets = true;
    assert_ne!(
        candidate.current_credential.cert_fingerprint, before.current_credential.cert_fingerprint,
        "replacement certificate must identify a distinct credential",
    );
    assert_eq!(candidate.last_seen_at, before.last_seen_at);
    let request = AdminClientInstanceUpdateRequest {
        display_name: Some(candidate.display_name.clone()),
        current_store_id: Some(candidate.current_store_id.clone()),
        status: Some(AdminClientInstanceStatus::Disabled),
        cert_pem: Some(replacement_cert),
        auto_create_wrapper_assets: Some(true),
    };

    let ((status, body), audit) = capture_tracing_output(|| {
        let _blocked = BlockedRegistryDir::new(&state.admin().clients_dir);
        run_handler(handle_admin_client_instance_update(
            State(state.clone()),
            Extension(principal.clone()),
            Path(client_id.clone()),
            Body(request.clone()),
        ))
    });
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["code"], "write_client_instances_failed");
    assert!(
        !audit.contains("action=client-instance.update"),
        "failed update emitted a success audit: {audit}",
    );
    assert_eq!(
        state
            .shared
            .client_instances
            .read()
            .unwrap()
            .get(&client_id),
        Some(&before),
    );
    let reloaded = crate::runtime_clients::load_client_instances(&state.admin().clients_dir)
        .expect("reload client registry after restoring its directory");
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded.get(&client_id), Some(&before));
    assert_eq!(
        load_policy_document(old_backend.as_ref()).unwrap(),
        policy_before,
        "failed remap must not start the old-store permission cascade",
    );

    let (retry_status, retry_body) = run_handler(handle_admin_client_instance_update(
        State(state.clone()),
        Extension(principal),
        Path(client_id.clone()),
        Body(request),
    ));
    assert_eq!(retry_status, StatusCode::OK, "{retry_body}");
    assert_eq!(
        state
            .shared
            .client_instances
            .read()
            .unwrap()
            .get(&client_id),
        Some(&candidate),
    );
    let reloaded = crate::runtime_clients::load_client_instances(&state.admin().clients_dir)
        .expect("reload client registry after successful retry");
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded.get(&client_id), Some(&candidate));

    let mut expected_policy = policy_before;
    expected_policy
        .permissions
        .retain(|row| row.client_instance != client_id);
    assert_eq!(
        load_policy_document(old_backend.as_ref()).unwrap(),
        expected_policy,
        "successful remap must remove the client's old-store permissions",
    );
}

#[test]
fn f015_client_delete_blocked_registry_preserves_record_and_permission_until_retry() {
    use underwrap_protocol::policy::WrapperApprovalMode;
    use underwrap_protocol::service::AdminWrapperPermissionUpsertRequest;

    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let old_store_id = create_store_for_test(
        state.clone(),
        &principal,
        &AdminStoreCreateRequest {
            name: "old".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    let (create_status, created) = run_handler(handle_admin_client_instance_create(
        State(state.clone()),
        Extension(principal.clone()),
        Body(AdminClientInstanceCreateRequest {
            display_name: "delete-candidate".into(),
            current_store_id: old_store_id.clone(),
            status: AdminClientInstanceStatus::Active,
            cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
            auto_create_wrapper_assets: Some(false),
        }),
    ));
    assert_eq!(create_status, StatusCode::OK, "{created}");
    let client_id = created["id"].as_str().unwrap().to_string();
    let record_path = state.admin().clients_dir.join(format!("{client_id}.json"));
    let before = state
        .shared
        .client_instances
        .read()
        .unwrap()
        .get(&client_id)
        .unwrap()
        .clone();

    let (resource_status, resource) = run_handler(handle_admin_resource_create(
        State(state.clone()),
        Extension(principal.clone()),
        Path(old_store_id.clone()),
        Body(AdminResourceCreateRequest {
            name: "F-015 delete resource".into(),
        }),
    ));
    assert_eq!(resource_status, StatusCode::OK, "{resource}");
    let resource_id = resource["resource"]["id"].as_str().unwrap().to_string();
    let (pair_status, pair) = run_handler(handle_admin_pair_create(
        State(state.clone()),
        Extension(principal.clone()),
        Path(old_store_id.clone()),
        Body(AdminPairCreateRequest {
            wrapper: "f015-delete-wrapper".into(),
            resource_id: resource_id.clone(),
        }),
    ));
    assert_eq!(pair_status, StatusCode::OK, "{pair}");
    let (permission_status, permission) = run_handler(handle_admin_wrapper_permission_upsert(
        State(state.clone()),
        Extension(principal.clone()),
        axum::extract::Path((
            old_store_id.clone(),
            client_id.clone(),
            "f015-delete-wrapper".into(),
            resource_id,
        )),
        Body(AdminWrapperPermissionUpsertRequest {
            mode: WrapperApprovalMode::Unlocked,
            writable: true,
            reason: Some("F-015 delete evidence".into()),
        }),
    ));
    assert_eq!(permission_status, StatusCode::OK, "{permission}");
    let old_backend = state
        .shared
        .stores
        .read()
        .unwrap()
        .expect_unlocked(&old_store_id);
    let policy_before = load_policy_document(old_backend.as_ref()).unwrap();
    assert_eq!(policy_before.permissions.len(), 1);
    assert_eq!(policy_before.permissions[0].client_instance, client_id);

    let ((status, body), audit) = capture_tracing_output(|| {
        let _blocked = BlockedRegistryDir::new(&state.admin().clients_dir);
        run_handler(handle_admin_client_instance_delete(
            State(state.clone()),
            Extension(principal.clone()),
            Path(client_id.clone()),
        ))
    });
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["code"], "write_client_instances_failed");
    assert!(
        !audit.contains("action=client-instance.delete"),
        "failed delete emitted a success audit: {audit}",
    );
    {
        let live = state.shared.client_instances.read().unwrap();
        assert_eq!(live.len(), 1);
        assert_eq!(live.get(&client_id), Some(&before));
    }
    assert!(record_path.exists());
    let reloaded = crate::runtime_clients::load_client_instances(&state.admin().clients_dir)
        .expect("reload client registry after restoring its directory");
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded.get(&client_id), Some(&before));
    assert_eq!(
        load_policy_document(old_backend.as_ref()).unwrap(),
        policy_before,
        "failed delete must not cascade the old-store permission",
    );

    let (retry_status, retry_body) = run_handler(handle_admin_client_instance_delete(
        State(state.clone()),
        Extension(principal),
        Path(client_id.clone()),
    ));
    assert_eq!(retry_status, StatusCode::OK, "{retry_body}");
    assert!(
        !state
            .shared
            .client_instances
            .read()
            .unwrap()
            .contains_key(&client_id),
    );
    assert!(!record_path.exists());
    assert!(
        !crate::runtime_clients::load_client_instances(&state.admin().clients_dir)
            .expect("reload client registry after successful delete retry")
            .contains_key(&client_id),
    );
    let mut expected_policy = policy_before;
    expected_policy
        .permissions
        .retain(|row| row.client_instance != client_id);
    assert_eq!(
        load_policy_document(old_backend.as_ref()).unwrap(),
        expected_policy,
        "successful delete retry must remove the old-store permission",
    );
}
