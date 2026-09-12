use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use base64::Engine;
use serde_json::Value;
use underwrap_protocol::policy::{
    ClientAccessOverride, POLICY_DOCUMENT_SECRET_ID, WrapperApprovalMode,
};
use underwrap_protocol::resource_blob::ResourceBlob;
use underwrap_protocol::service::{
    AdminClientInstanceUpdateRequest, RuntimeWrapperInvocationFinalizeRequest,
    RuntimeWrapperInvocationOpenRequest,
};
use underwrap_protocol::variant::WrapperVariant;

use crate::admin_api::client_instances::handle_admin_client_instance_update;
use crate::http::responses::Body;
use crate::runtime_api::{
    PeerCertificates, ResourceBody, axum_invocation_finalize, axum_invocation_open,
};
use crate::runtime_clients::{ClientInstanceStatus, load_client_instances, save_client_instance};
use crate::store_registry::StoreState;
use crate::test_support::{
    BlockedRegistryDir, RuntimeApiFixture, admin_principal, make_runtime_api_fixture, run_handler,
};

// Exercises request-time authentication/lease/handler resolution with a simulated
// certificate extension. Real TLS and daemon behavior belong to integration tests.
fn fixture() -> (RuntimeApiFixture, PeerCertificates) {
    let fx = make_runtime_api_fixture();
    let certs = {
        let mut instances = fx.state.shared.client_instances.write().unwrap();
        let record = instances.get_mut(&fx.instance_id).unwrap();
        let certs = rustls_pemfile::certs(&mut record.current_credential.cert_pem.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        record.current_credential.cert_fingerprint =
            crate::runtime_clients::cert_fingerprint_from_der(certs[0].as_ref());
        save_client_instance(&fx.state.admin().clients_dir, record).unwrap();
        certs
    };
    (fx, PeerCertificates(Arc::new(certs)))
}

fn update(fx: &RuntimeApiFixture, access_override: ClientAccessOverride) -> (StatusCode, Value) {
    run_handler(handle_admin_client_instance_update(
        State(fx.state.clone()),
        Extension(admin_principal()),
        Path(fx.instance_id.clone()),
        Body(AdminClientInstanceUpdateRequest {
            access_override: Some(access_override),
            ..Default::default()
        }),
    ))
}

fn open(
    fx: &RuntimeApiFixture,
    peer: &PeerCertificates,
    variant: WrapperVariant,
) -> (StatusCode, Value) {
    run_handler(axum_invocation_open(
        State(fx.state.clone()),
        Extension(peer.clone()),
        Body(RuntimeWrapperInvocationOpenRequest {
            wrapper_name: "gh".into(),
            resource_name: "alpha".into(),
            variant,
        }),
    ))
}

fn finalize(
    fx: &RuntimeApiFixture,
    peer: &PeerCertificates,
    variant: WrapperVariant,
    blob: Option<&ResourceBlob>,
) -> (StatusCode, Value) {
    run_handler(axum_invocation_finalize(
        State(fx.state.clone()),
        Extension(peer.clone()),
        ResourceBody(RuntimeWrapperInvocationFinalizeRequest {
            wrapper_name: "gh".into(),
            resource_name: "alpha".into(),
            variant,
            blob_b64: blob.map(|blob| {
                base64::engine::general_purpose::STANDARD.encode(blob.to_canonical_bytes().unwrap())
            }),
        }),
    ))
}

#[test]
fn live_overrides_apply_between_requests_and_preserve_policy_bytes() {
    let (fx, peer) = fixture();
    let store = fx.store();
    let before = store.read(POLICY_DOCUMENT_SECRET_ID).unwrap();
    assert_eq!(open(&fx, &peer, WrapperVariant::Real).1["outcome"], "allow");

    for (access, outcome, writable) in [
        (ClientAccessOverride::Locked {}, "deny", false),
        (
            ClientAccessOverride::Unlocked { writable: false },
            "allow",
            false,
        ),
        (
            ClientAccessOverride::Unlocked { writable: true },
            "allow",
            true,
        ),
        (ClientAccessOverride::Individual {}, "allow", true),
    ] {
        let (status, body) = update(&fx, access);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["access_override"],
            serde_json::to_value(access).unwrap()
        );
        let reloaded = load_client_instances(&fx.state.admin().clients_dir).unwrap();
        assert_eq!(reloaded[&fx.instance_id].access_override, access);
        let (status, body) = open(&fx, &peer, WrapperVariant::Real);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["outcome"], outcome);
        if outcome == "allow" {
            assert_eq!(body["writable"], writable);
        }
        let (_, finalized) = finalize(
            &fx,
            &peer,
            WrapperVariant::Real,
            Some(&ResourceBlob::empty()),
        );
        assert_eq!(finalized["outcome"], if writable { "ok" } else { "deny" });
        let (_, usage) = finalize(&fx, &peer, WrapperVariant::Real, None);
        assert_eq!(
            usage["outcome"],
            if outcome == "allow" { "ok" } else { "deny" }
        );
        assert!(store.read(POLICY_DOCUMENT_SECRET_ID).unwrap() == before);
    }
}

#[test]
fn unlock_does_not_create_or_extend_debug_approval_or_modify_saved_flags() {
    let (fx, peer) = fixture();
    for mode in [
        None,
        Some(WrapperApprovalMode::Locked),
        Some(WrapperApprovalMode::UnlockedUntil { until_epoch: 1 }),
        Some(WrapperApprovalMode::UnsafeDebugUntil { until_epoch: 1 }),
        Some(WrapperApprovalMode::UnsafeDebugUntil {
            until_epoch: u64::MAX,
        }),
    ] {
        let debug_allowed = matches!(
            mode,
            Some(WrapperApprovalMode::UnsafeDebugUntil {
                until_epoch: u64::MAX
            })
        );
        let mut policy = fx.policy();
        policy.permissions.clear();
        if let Some(mode) = mode {
            policy
                .permissions
                .push(underwrap_protocol::policy::WrapperPermission {
                    client_instance: fx.instance_id.clone(),
                    wrapper: "gh".into(),
                    resource_id: fx.resource_id.clone(),
                    mode,
                    writable: false,
                    reason: None,
                });
        }
        fx.save_policy(&policy);
        for writable in [false, true] {
            assert_eq!(
                update(&fx, ClientAccessOverride::Unlocked { writable }).0,
                StatusCode::OK
            );
            assert_eq!(open(&fx, &peer, WrapperVariant::Real).1["outcome"], "allow");
            let (_, debug) = open(&fx, &peer, WrapperVariant::Debug);
            assert_eq!(
                debug["outcome"],
                if debug_allowed { "allow" } else { "deny" }
            );
            if debug_allowed {
                assert_eq!(debug["writable"], writable);
            }
            let (_, debug_finalized) = finalize(
                &fx,
                &peer,
                WrapperVariant::Debug,
                Some(&ResourceBlob::empty()),
            );
            assert_eq!(
                debug_finalized["outcome"],
                if debug_allowed && writable {
                    "ok"
                } else {
                    "deny"
                }
            );
            assert_eq!(
                update(&fx, ClientAccessOverride::Locked {}).0,
                StatusCode::OK
            );
            assert_eq!(
                open(&fx, &peer, WrapperVariant::Debug).1["reason"],
                "client-locked"
            );
            assert_eq!(
                finalize(&fx, &peer, WrapperVariant::Debug, None).1["reason"],
                "client-locked"
            );
            assert!(fx.policy() == policy);
        }
        assert_eq!(
            update(&fx, ClientAccessOverride::Individual {}).0,
            StatusCode::OK
        );
        assert_eq!(
            open(&fx, &peer, WrapperVariant::Real).1["outcome"],
            if debug_allowed { "allow" } else { "deny" }
        );
    }
}

#[test]
fn failed_override_persistence_leaves_live_mode_and_policy_unchanged() {
    let (fx, peer) = fixture();
    let before = fx.policy();
    {
        let _blocked = BlockedRegistryDir::new(&fx.state.admin().clients_dir);
        assert_eq!(
            update(&fx, ClientAccessOverride::Locked {}).0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(open(&fx, &peer, WrapperVariant::Real).1["outcome"], "allow");
    }
    let loaded = load_client_instances(&fx.state.admin().clients_dir).unwrap();
    assert_eq!(
        loaded[&fx.instance_id].access_override,
        ClientAccessOverride::Individual {}
    );
    assert!(fx.policy() == before);
}

#[test]
fn unlock_preserves_identity_registration_and_store_availability_gates() {
    let (fx, peer) = fixture();
    assert_eq!(
        update(&fx, ClientAccessOverride::Unlocked { writable: true }).0,
        StatusCode::OK
    );
    fx.state
        .shared
        .client_instances
        .write()
        .unwrap()
        .get_mut(&fx.instance_id)
        .unwrap()
        .status = ClientInstanceStatus::Disabled;
    assert_eq!(
        open(&fx, &peer, WrapperVariant::Real).0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        finalize(&fx, &peer, WrapperVariant::Real, None).0,
        StatusCode::UNAUTHORIZED
    );
    fx.state
        .shared
        .client_instances
        .write()
        .unwrap()
        .get_mut(&fx.instance_id)
        .unwrap()
        .status = ClientInstanceStatus::Active;

    crate::storage::documents::save_pairs_document(fx.store().as_ref(), &Default::default())
        .unwrap();
    assert_eq!(
        open(&fx, &peer, WrapperVariant::Real).1["reason"],
        "unregistered-pair"
    );
    assert_eq!(
        finalize(&fx, &peer, WrapperVariant::Real, None).1["reason"],
        "unregistered-pair"
    );

    // Simulated lifecycle states check lease admission, not an actual migration/lock.
    for state in [
        StoreState::Locked,
        StoreState::Busy,
        StoreState::RecoveryRequired,
    ] {
        fx.state
            .shared
            .stores
            .write()
            .unwrap()
            .entries
            .get_mut(&fx.store_id)
            .unwrap()
            .state = state;
        assert!(!open(&fx, &peer, WrapperVariant::Real).0.is_success());
        assert!(
            !finalize(&fx, &peer, WrapperVariant::Real, None)
                .0
                .is_success()
        );
    }
}

#[test]
fn writable_unlock_still_rejects_env_mutation() {
    let (fx, peer) = fixture();
    assert_eq!(
        update(&fx, ClientAccessOverride::Unlocked { writable: true }).0,
        StatusCode::OK
    );
    let mut blob = ResourceBlob::empty();
    blob.env.insert("NEW_ENTRY".into(), vec![42_u8; 8].into());
    let (_, body) = finalize(&fx, &peer, WrapperVariant::Real, Some(&blob));
    assert_eq!(body["outcome"], "invalid-blob");
    assert_eq!(body["reason"], "env-section-immutable");
    assert!(
        underwrap_backend_store::load_resource_blob(fx.store().as_ref(), &fx.resource_id)
            .unwrap()
            .env
            .is_empty()
    );
}
