use super::*;
use crate::storage::documents::{load_pairs_document, save_pairs_document};
use crate::test_support::{
    entity_test_state_with_store, make_runtime_api_fixture, response_body_json,
};

#[test]
fn bump_pair_last_used_sets_timestamp_only_on_matching_pair() {
    use underwrap_protocol::entities::{EntityCreator, WrapperResourcePairRecord};
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let store_arc = state
        .shared
        .stores
        .read()
        .unwrap()
        .expect_unlocked(&store_id);
    let store: &dyn StorageBackend = store_arc.as_ref();

    let mut pairs = load_pairs_document(store).unwrap();
    pairs
        .insert(WrapperResourcePairRecord {
            wrapper: "gh".into(),
            resource_id: "res_token".into(),
            created_by: EntityCreator::ClientInstance {
                id: "inst_a".into(),
            },
            created_at: 100,
            last_used_at: None,
        })
        .unwrap();
    pairs
        .insert(WrapperResourcePairRecord {
            wrapper: "tf".into(),
            resource_id: "res_token".into(),
            created_by: EntityCreator::ClientInstance {
                id: "inst_a".into(),
            },
            created_at: 100,
            last_used_at: None,
        })
        .unwrap();
    save_pairs_document(store, &pairs).unwrap();

    bump_pair_last_used(store, "gh", "res_token").unwrap();

    let after = load_pairs_document(store).unwrap();
    let gh = after.find("gh", "res_token").unwrap();
    let tf = after.find("tf", "res_token").unwrap();
    assert!(gh.last_used_at.is_some(), "matched pair should be bumped");
    assert!(tf.last_used_at.is_none(), "other pair must not be bumped");
}

#[test]
fn bump_pair_last_used_silent_when_no_pair() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (state, store_id) = entity_test_state_with_store(tmp.path());
    let store_arc = state
        .shared
        .stores
        .read()
        .unwrap()
        .expect_unlocked(&store_id);
    // No pair record for (gh, res_token) → no-op, no error.
    bump_pair_last_used(store_arc.as_ref(), "gh", "res_token").unwrap();
}

#[test]
fn runtime_wrapper_invocation_open_allows_unlocked_permission() {
    use base64::Engine;
    use underwrap_protocol::resource_blob::ResourceBlob;
    use underwrap_protocol::service::{
        RuntimeWrapperInvocationOpenRequest, RuntimeWrapperInvocationOpenResponse,
    };

    let fx = make_runtime_api_fixture();
    let resp = handle_runtime_wrapper_invocation_open(
        fx.store().as_ref(),
        &fx.instance_id,
        &RuntimeWrapperInvocationOpenRequest {
            wrapper_name: "gh".into(),
            resource_name: "alpha".into(),
        },
    );
    let parsed: RuntimeWrapperInvocationOpenResponse =
        serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
    match parsed {
        RuntimeWrapperInvocationOpenResponse::Allow {
            resource_id,
            blob_b64,
            writable,
        } => {
            assert!(writable, "fixture seeds writable=true");
            assert_eq!(resource_id, fx.resource_id);
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&blob_b64)
                .unwrap();
            let blob = ResourceBlob::from_canonical_bytes(&bytes).unwrap();
            assert!(blob.env.is_empty());
            assert!(blob.files.is_empty());
        }
        other => panic!("expected Allow, got {other:?}"),
    }
}

#[test]
fn runtime_wrapper_invocation_open_denies_locked_permission() {
    use underwrap_protocol::policy::WrapperApprovalMode;
    use underwrap_protocol::service::{
        RuntimeWrapperInvocationOpenRequest, RuntimeWrapperInvocationOpenResponse,
    };

    let fx = make_runtime_api_fixture();
    let mut policy = fx.policy();
    policy.permissions[0].mode = WrapperApprovalMode::Locked;
    fx.save_policy(&policy);
    let resp = handle_runtime_wrapper_invocation_open(
        fx.store().as_ref(),
        &fx.instance_id,
        &RuntimeWrapperInvocationOpenRequest {
            wrapper_name: "gh".into(),
            resource_name: "alpha".into(),
        },
    );
    let parsed: RuntimeWrapperInvocationOpenResponse =
        serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
    match parsed {
        RuntimeWrapperInvocationOpenResponse::Deny { reason } => {
            assert_eq!(reason, "locked");
        }
        other => panic!("expected Deny(locked), got {other:?}"),
    }
}

#[test]
fn runtime_wrapper_invocation_open_denies_unregistered_pair() {
    use underwrap_protocol::service::{
        RuntimeWrapperInvocationOpenRequest, RuntimeWrapperInvocationOpenResponse,
    };

    let fx = make_runtime_api_fixture();
    let resp = handle_runtime_wrapper_invocation_open(
        fx.store().as_ref(),
        &fx.instance_id,
        &RuntimeWrapperInvocationOpenRequest {
            wrapper_name: "ghost".into(),
            resource_name: "alpha".into(),
        },
    );
    let parsed: RuntimeWrapperInvocationOpenResponse =
        serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
    assert!(matches!(
        parsed,
        RuntimeWrapperInvocationOpenResponse::Deny { reason } if reason == "unregistered-pair"
    ));
}

#[test]
fn runtime_wrapper_invocation_finalize_bumps_last_used_without_blob() {
    use underwrap_protocol::service::{
        RuntimeWrapperInvocationFinalizeRequest, RuntimeWrapperInvocationFinalizeResponse,
    };

    let fx = make_runtime_api_fixture();
    let resp = handle_runtime_wrapper_invocation_finalize(
        fx.store().as_ref(),
        &fx.instance_id,
        &RuntimeWrapperInvocationFinalizeRequest {
            wrapper_name: "gh".into(),
            resource_name: "alpha".into(),
            blob_b64: None,
        },
    );
    let parsed: RuntimeWrapperInvocationFinalizeResponse =
        serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
    assert_eq!(parsed, RuntimeWrapperInvocationFinalizeResponse::Ok);
    let pairs = load_pairs_document(fx.store().as_ref()).unwrap();
    assert!(
        pairs
            .find("gh", &fx.resource_id)
            .unwrap()
            .last_used_at
            .is_some(),
    );
}

#[test]
fn runtime_wrapper_invocation_finalize_persists_libsecret_change() {
    use base64::Engine;
    use underwrap_protocol::resource_blob::{Collection, ResourceBlob};
    use underwrap_protocol::service::{
        RuntimeWrapperInvocationFinalizeRequest, RuntimeWrapperInvocationFinalizeResponse,
    };

    let fx = make_runtime_api_fixture();
    let mut blob = ResourceBlob::empty();
    blob.libsecret.collections.insert(
        "default".into(),
        Collection {
            label: "Default".into(),
            created: 1,
            modified: 2,
            items: Default::default(),
        },
    );
    let bytes = blob.to_canonical_bytes().unwrap();
    let resp = handle_runtime_wrapper_invocation_finalize(
        fx.store().as_ref(),
        &fx.instance_id,
        &RuntimeWrapperInvocationFinalizeRequest {
            wrapper_name: "gh".into(),
            resource_name: "alpha".into(),
            blob_b64: Some(base64::engine::general_purpose::STANDARD.encode(&bytes)),
        },
    );
    let parsed: RuntimeWrapperInvocationFinalizeResponse =
        serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
    assert_eq!(parsed, RuntimeWrapperInvocationFinalizeResponse::Ok);
    let stored = load_resource_blob(fx.store().as_ref(), &fx.resource_id).unwrap();
    assert!(stored.libsecret.collections.contains_key("default"));
}

#[test]
fn runtime_wrapper_invocation_finalize_rejects_env_mutation() {
    use base64::Engine;
    use underwrap_protocol::message::SecretBytes;
    use underwrap_protocol::resource_blob::ResourceBlob;
    use underwrap_protocol::service::{
        RuntimeWrapperInvocationFinalizeRequest, RuntimeWrapperInvocationFinalizeResponse,
    };

    let fx = make_runtime_api_fixture();
    // Seed an env entry in the stored blob.
    let mut stored = ResourceBlob::empty();
    stored
        .env
        .insert("TOKEN".into(), SecretBytes::from(b"abc".to_vec()));
    save_resource_blob(fx.store().as_ref(), &fx.resource_id, &stored).unwrap();

    // Wrapper tries to rewrite TOKEN.
    let mut mutated = stored.clone();
    mutated
        .env
        .insert("TOKEN".into(), SecretBytes::from(b"xyz".to_vec()));
    let bytes = mutated.to_canonical_bytes().unwrap();
    let resp = handle_runtime_wrapper_invocation_finalize(
        fx.store().as_ref(),
        &fx.instance_id,
        &RuntimeWrapperInvocationFinalizeRequest {
            wrapper_name: "gh".into(),
            resource_name: "alpha".into(),
            blob_b64: Some(base64::engine::general_purpose::STANDARD.encode(&bytes)),
        },
    );
    let parsed: RuntimeWrapperInvocationFinalizeResponse =
        serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
    assert!(matches!(
        parsed,
        RuntimeWrapperInvocationFinalizeResponse::InvalidBlob { reason }
            if reason == "env-section-immutable"
    ));
    // Stored env section must be untouched.
    let after = load_resource_blob(fx.store().as_ref(), &fx.resource_id).unwrap();
    assert_eq!(
        after.env.get("TOKEN").map(|v| v.as_ref().to_vec()),
        Some(b"abc".to_vec())
    );
}

#[test]
fn runtime_wrapper_invocation_finalize_rejects_noncanonical_blob() {
    use base64::Engine;
    use underwrap_protocol::service::{
        RuntimeWrapperInvocationFinalizeRequest, RuntimeWrapperInvocationFinalizeResponse,
    };

    let fx = make_runtime_api_fixture();
    // Hand-craft a valid-but-noncanonical JSON: trailing whitespace
    // around the body keeps the bytes parseable but is rejected by
    // the round-trip equality check.
    let noncanonical = br#"{"schema_version":1,"env":{},"files":{},"libsecret":{"aliases":{},"collections":{}}}  "#;
    let resp = handle_runtime_wrapper_invocation_finalize(
        fx.store().as_ref(),
        &fx.instance_id,
        &RuntimeWrapperInvocationFinalizeRequest {
            wrapper_name: "gh".into(),
            resource_name: "alpha".into(),
            blob_b64: Some(
                base64::engine::general_purpose::STANDARD.encode(noncanonical.as_slice()),
            ),
        },
    );
    let parsed: RuntimeWrapperInvocationFinalizeResponse =
        serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
    assert!(matches!(
        parsed,
        RuntimeWrapperInvocationFinalizeResponse::InvalidBlob { reason }
            if reason == "noncanonical-blob"
    ));
}

#[test]
fn runtime_wrapper_invocation_finalize_rejects_writeback_when_not_writable() {
    use base64::Engine;
    use underwrap_protocol::resource_blob::ResourceBlob;
    use underwrap_protocol::service::{
        RuntimeWrapperInvocationFinalizeRequest, RuntimeWrapperInvocationFinalizeResponse,
    };

    let fx = make_runtime_api_fixture();
    let mut policy = fx.policy();
    policy.permissions[0].writable = false;
    fx.save_policy(&policy);

    let bytes = ResourceBlob::empty().to_canonical_bytes().unwrap();
    let resp = handle_runtime_wrapper_invocation_finalize(
        fx.store().as_ref(),
        &fx.instance_id,
        &RuntimeWrapperInvocationFinalizeRequest {
            wrapper_name: "gh".into(),
            resource_name: "alpha".into(),
            blob_b64: Some(base64::engine::general_purpose::STANDARD.encode(&bytes)),
        },
    );
    let parsed: RuntimeWrapperInvocationFinalizeResponse =
        serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
    assert!(matches!(
        parsed,
        RuntimeWrapperInvocationFinalizeResponse::Deny { reason } if reason == "not-writable"
    ));
}
