//! Direct mTLS coverage of the three runtime endpoints
//! (`POST /v1/wrappers`, `POST /v1/wrapper-invocations`,
//! `POST /v1/wrapper-invocations/finalize`). The daemon is started in
//! service mode but every request is issued via
//! [`TestDaemon::direct_service_client`] so the tests pin the backend's
//! behaviour independently of the daemon — i.e. they double as
//! daemon-bypass trust-boundary tests.
//!
//! Higher-level flow coverage (via the wrapper socket, the daemon, and
//! frontend-exec) lives in `wrapper_permissions.rs`,
//! `wrapper_writeback.rs`, `last_used.rs`, and the rest of the suite.
//! The split keeps this file readable: every test here is one request
//! and one response assertion.
use super::*;
use base64::Engine;
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
use underwrap_protocol::resource_blob::ResourceBlob;
use underwrap_protocol::service::{
    AdminClientInstanceUpdateRequest, RuntimeWrapperInvocationFinalizeRequest,
    RuntimeWrapperInvocationFinalizeResponse, RuntimeWrapperInvocationOpenRequest,
    RuntimeWrapperInvocationOpenResponse, RuntimeWrapperRegisterPair,
    RuntimeWrapperRegisterRequest, RuntimeWrapperRegisterResponse,
};
use underwrap_protocol::variant::WrapperVariant;

// ---------------------------------------------------------------------------
// Direct-call helpers
// ---------------------------------------------------------------------------

async fn direct_register(
    daemon: &TestDaemon,
    pairs: &[(&str, &str)],
) -> RuntimeWrapperRegisterResponse {
    let client = daemon.direct_service_client().unwrap();
    let resp = client
        .post(format!("{}/v1/wrappers", daemon.service_url()))
        .json(&RuntimeWrapperRegisterRequest {
            wrappers: pairs
                .iter()
                .map(|(w, r)| RuntimeWrapperRegisterPair {
                    wrapper_name: (*w).to_string(),
                    resource_name: (*r).to_string(),
                })
                .collect(),
        })
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "register returned non-2xx: {} body={}",
        resp.status(),
        resp.text().await.unwrap_or_default(),
    );
    resp.json().await.unwrap()
}

async fn direct_open(
    daemon: &TestDaemon,
    wrapper: &str,
    resource: &str,
) -> RuntimeWrapperInvocationOpenResponse {
    let client = daemon.direct_service_client().unwrap();
    let resp = client
        .post(format!("{}/v1/wrapper-invocations", daemon.service_url()))
        .json(&RuntimeWrapperInvocationOpenRequest {
            wrapper_name: wrapper.to_string(),
            resource_name: resource.to_string(),
            variant: WrapperVariant::Real,
        })
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "open returned non-2xx: {} body={}",
        resp.status(),
        resp.text().await.unwrap_or_default(),
    );
    resp.json().await.unwrap()
}

async fn direct_finalize(
    daemon: &TestDaemon,
    wrapper: &str,
    resource: &str,
    blob_b64: Option<String>,
) -> RuntimeWrapperInvocationFinalizeResponse {
    let client = daemon.direct_service_client().unwrap();
    let resp = client
        .post(format!(
            "{}/v1/wrapper-invocations/finalize",
            daemon.service_url()
        ))
        .json(&RuntimeWrapperInvocationFinalizeRequest {
            wrapper_name: wrapper.to_string(),
            resource_name: resource.to_string(),
            variant: WrapperVariant::Real,
            blob_b64,
        })
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "finalize returned non-2xx: {} body={}",
        resp.status(),
        resp.text().await.unwrap_or_default(),
    );
    resp.json().await.unwrap()
}

fn blob_b64(blob: &ResourceBlob) -> String {
    let bytes = blob.to_canonical_bytes().unwrap();
    base64::engine::general_purpose::STANDARD.encode(&bytes)
}

async fn allow_blob(daemon: &TestDaemon, wrapper: &str, resource: &str) -> String {
    match direct_open(daemon, wrapper, resource).await {
        RuntimeWrapperInvocationOpenResponse::Allow { blob_b64, .. } => blob_b64,
        other => panic!("expected Allow from open, got {other:?}"),
    }
}

fn decoded_blob(blob_b64: &str) -> ResourceBlob {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(blob_b64)
        .unwrap();
    ResourceBlob::from_canonical_bytes(&bytes).unwrap()
}

async fn set_auto_create(daemon: &TestDaemon, enable: bool) {
    let body = AdminClientInstanceUpdateRequest {
        display_name: None,
        current_store_id: None,
        status: None,
        cert_pem: None,
        auto_create_wrapper_assets: Some(enable),
    };
    daemon
        .update_client_instance(daemon.client_instance(), &body)
        .await
        .unwrap();
}

// ===========================================================================
// `POST /v1/wrappers` — register
// ===========================================================================

#[tokio::test]
async fn register_with_empty_body_succeeds_without_creating_anything() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    match direct_register(&daemon, &[]).await {
        RuntimeWrapperRegisterResponse::Ok {
            auto_created_resources,
            auto_created_pairs,
        } => {
            assert!(auto_created_resources.is_empty());
            assert!(auto_created_pairs.is_empty());
        }
        other => panic!("expected Ok, got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn register_refuses_when_auto_create_disabled_and_pair_is_unknown() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    // Default for fresh client instances is auto_create=false. Sanity-set
    // it anyway so this test doesn't depend on the default.
    set_auto_create(&daemon, false).await;

    let pairs = &[("gh-authorized", "github/token")];
    match direct_register(&daemon, pairs).await {
        RuntimeWrapperRegisterResponse::AutoCreateDisabled {
            missing_resources,
            missing_pairs,
        } => {
            assert_eq!(missing_resources, vec!["github/token".to_string()]);
            assert_eq!(missing_pairs.len(), 1);
            assert_eq!(missing_pairs[0].wrapper_name, "gh-authorized");
            assert_eq!(missing_pairs[0].resource_name, "github/token");
        }
        other => panic!("expected AutoCreateDisabled, got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn register_with_auto_create_enabled_creates_resource_and_pair() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    set_auto_create(&daemon, true).await;

    match direct_register(&daemon, &[("gh-authorized", "github/token")]).await {
        RuntimeWrapperRegisterResponse::Ok {
            auto_created_resources,
            auto_created_pairs,
        } => {
            assert_eq!(auto_created_resources, vec!["github/token".to_string()]);
            assert_eq!(auto_created_pairs.len(), 1);
            assert_eq!(auto_created_pairs[0].wrapper_name, "gh-authorized");
        }
        other => panic!("expected Ok, got {other:?}"),
    }

    // Re-registering the same pair must report empty creation lists
    // (idempotent — the backend matches by name/wrapper and returns the
    // no-op fast path).
    match direct_register(&daemon, &[("gh-authorized", "github/token")]).await {
        RuntimeWrapperRegisterResponse::Ok {
            auto_created_resources,
            auto_created_pairs,
        } => {
            assert!(auto_created_resources.is_empty());
            assert!(auto_created_pairs.is_empty());
        }
        other => panic!("expected Ok on idempotent re-register, got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn register_rejects_empty_wrapper_or_resource_name() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    let client = daemon.direct_service_client().unwrap();
    for (w, r) in [("", "github/token"), ("gh-authorized", ""), ("", "")] {
        let resp = client
            .post(format!("{}/v1/wrappers", daemon.service_url()))
            .json(&RuntimeWrapperRegisterRequest {
                wrappers: vec![RuntimeWrapperRegisterPair {
                    wrapper_name: w.to_string(),
                    resource_name: r.to_string(),
                }],
            })
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            reqwest::StatusCode::BAD_REQUEST,
            "empty (wrapper={w:?}, resource={r:?}) must 400, got {}",
            resp.status(),
        );
    }
    daemon.stop().await.unwrap();
}

// ===========================================================================
// `POST /v1/wrapper-invocations` — open
// ===========================================================================

#[tokio::test]
async fn open_returns_blob_and_resource_id_for_unlocked_permission() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_abc")
        .await
        .unwrap();
    let permission = daemon.current_permission("gh-authorized", "github/token");
    daemon.upsert_permission("test", &permission).await.unwrap();

    match direct_open(&daemon, "gh-authorized", "github/token").await {
        RuntimeWrapperInvocationOpenResponse::Allow {
            resource_id,
            blob_b64,
            writable,
        } => {
            // The harness seeds resource_id == resource_name for
            // string-keyed fixtures, so we can pin the value here.
            assert_eq!(resource_id, "github/token");
            assert!(!writable, "default permission is read-only");
            let blob = decoded_blob(&blob_b64);
            // store_secret writes via admin to the env slot.
            assert!(!blob.env.is_empty());
        }
        other => panic!("expected Allow, got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn open_denies_with_unregistered_pair_for_unknown_resource_name() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    match direct_open(&daemon, "gh-authorized", "does/not/exist").await {
        RuntimeWrapperInvocationOpenResponse::Deny { reason } => {
            assert_eq!(reason, "unregistered-pair");
        }
        other => panic!("expected Deny, got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn open_denies_with_no_permission_when_pair_exists_but_policy_is_empty() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    // Seed the resource + pair records directly so the pair is
    // "registered" but the policy stays empty.
    daemon.ensure_pair("gh-authorized", "github/token").unwrap();
    daemon
        .put_policy("test", &PolicyDocument::default())
        .await
        .unwrap();

    match direct_open(&daemon, "gh-authorized", "github/token").await {
        RuntimeWrapperInvocationOpenResponse::Deny { reason } => {
            assert_eq!(reason, "no-permission");
        }
        other => panic!("expected Deny, got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn open_denies_with_reason_for_locked_permission() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_abc")
        .await
        .unwrap();
    daemon
        .upsert_permission(
            "test",
            &WrapperPermission {
                client_instance: daemon.client_instance().to_string(),
                wrapper: "gh-authorized".into(),
                resource_id: "github/token".into(),
                mode: WrapperApprovalMode::Locked,
                writable: false,
                reason: Some("scheduled maintenance".into()),
            },
        )
        .await
        .unwrap();

    match direct_open(&daemon, "gh-authorized", "github/token").await {
        RuntimeWrapperInvocationOpenResponse::Deny { reason } => {
            // The backend surfaces the permission's reason verbatim
            // when set; otherwise falls back to "locked".
            assert_eq!(reason, "scheduled maintenance");
        }
        other => panic!("expected Deny, got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn open_denies_with_expired_for_unlocked_until_in_the_past() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_abc")
        .await
        .unwrap();
    daemon
        .upsert_permission(
            "test",
            &WrapperPermission {
                client_instance: daemon.client_instance().to_string(),
                wrapper: "gh-authorized".into(),
                resource_id: "github/token".into(),
                mode: WrapperApprovalMode::UnlockedUntil { until_epoch: 1 },
                writable: false,
                reason: None,
            },
        )
        .await
        .unwrap();

    match direct_open(&daemon, "gh-authorized", "github/token").await {
        RuntimeWrapperInvocationOpenResponse::Deny { reason } => {
            assert_eq!(reason, "expired");
        }
        other => panic!("expected Deny, got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn open_rejects_empty_wrapper_or_resource_name() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    let client = daemon.direct_service_client().unwrap();
    for (w, r) in [("", "github/token"), ("gh-authorized", "")] {
        let resp = client
            .post(format!("{}/v1/wrapper-invocations", daemon.service_url()))
            .json(&RuntimeWrapperInvocationOpenRequest {
                wrapper_name: w.to_string(),
                resource_name: r.to_string(),
                variant: WrapperVariant::Real,
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    }
    daemon.stop().await.unwrap();
}

// ===========================================================================
// `POST /v1/wrapper-invocations/finalize` — usage signal + writeback
// ===========================================================================

#[tokio::test]
async fn finalize_without_blob_is_ok_even_when_permission_is_readonly() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_abc")
        .await
        .unwrap();
    // writable=false on purpose — usage-signal finalize must still
    // succeed regardless of writable.
    let permission = daemon.current_permission("gh-authorized", "github/token");
    daemon.upsert_permission("test", &permission).await.unwrap();

    let resp = direct_finalize(&daemon, "gh-authorized", "github/token", None).await;
    assert!(
        matches!(resp, RuntimeWrapperInvocationFinalizeResponse::Ok),
        "expected Ok, got {resp:?}"
    );
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn finalize_rejects_blob_whose_env_section_was_mutated() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_initial")
        .await
        .unwrap();
    let mut permission = daemon.current_permission("gh-authorized", "github/token");
    permission.writable = true;
    daemon.upsert_permission("test", &permission).await.unwrap();

    // Fetch the current canonical blob, then mutate the env section.
    let current_b64 = allow_blob(&daemon, "gh-authorized", "github/token").await;
    let mut blob = decoded_blob(&current_b64);
    blob.env
        .insert("INJECTED".to_string(), SecretBytes::from(b"value".to_vec()));

    let resp = direct_finalize(
        &daemon,
        "gh-authorized",
        "github/token",
        Some(blob_b64(&blob)),
    )
    .await;
    match resp {
        RuntimeWrapperInvocationFinalizeResponse::InvalidBlob { reason } => {
            assert_eq!(reason, "env-section-immutable");
        }
        other => panic!("expected InvalidBlob(env-section-immutable), got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn finalize_accepts_blob_with_files_section_modified_and_env_unchanged() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_initial")
        .await
        .unwrap();
    let mut permission = daemon.current_permission("gh-authorized", "github/token");
    permission.writable = true;
    daemon.upsert_permission("test", &permission).await.unwrap();

    let current_b64 = allow_blob(&daemon, "gh-authorized", "github/token").await;
    let mut blob = decoded_blob(&current_b64);
    blob.files.insert(
        ".config/gh/hosts.yml".to_string(),
        SecretBytes::from(b"file contents".to_vec()),
    );

    let resp = direct_finalize(
        &daemon,
        "gh-authorized",
        "github/token",
        Some(blob_b64(&blob)),
    )
    .await;
    assert!(
        matches!(resp, RuntimeWrapperInvocationFinalizeResponse::Ok),
        "files-only mutation must be accepted, got {resp:?}",
    );

    // Round-trip: a follow-up open returns the new files section.
    let after_b64 = allow_blob(&daemon, "gh-authorized", "github/token").await;
    let after = decoded_blob(&after_b64);
    assert!(after.files.contains_key(".config/gh/hosts.yml"));
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn finalize_with_blob_on_readonly_permission_is_denied_not_invalid() {
    // The writable check fires BEFORE blob validation, so a readonly
    // permission gets Deny{not-writable}, not InvalidBlob. This pins
    // the order: never report "your blob is bad" to a client who isn't
    // even allowed to write.
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_initial")
        .await
        .unwrap();
    let permission = daemon.current_permission("gh-authorized", "github/token");
    daemon.upsert_permission("test", &permission).await.unwrap();

    let resp = direct_finalize(
        &daemon,
        "gh-authorized",
        "github/token",
        // Garbage bytes — would normally surface decode-failed, but
        // the writable check fires first.
        Some(base64::engine::general_purpose::STANDARD.encode(b"not a blob")),
    )
    .await;
    match resp {
        RuntimeWrapperInvocationFinalizeResponse::Deny { reason } => {
            assert_eq!(reason, "not-writable");
        }
        other => panic!("expected Deny(not-writable), got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn finalize_rejects_undecodable_blob_bytes() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_initial")
        .await
        .unwrap();
    let mut permission = daemon.current_permission("gh-authorized", "github/token");
    permission.writable = true;
    daemon.upsert_permission("test", &permission).await.unwrap();

    let resp = direct_finalize(
        &daemon,
        "gh-authorized",
        "github/token",
        Some(base64::engine::general_purpose::STANDARD.encode(b"not a blob")),
    )
    .await;
    match resp {
        RuntimeWrapperInvocationFinalizeResponse::InvalidBlob { reason } => {
            assert_eq!(reason, "decode-failed");
        }
        other => panic!("expected InvalidBlob(decode-failed), got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn finalize_rejects_unknown_schema_version() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_initial")
        .await
        .unwrap();
    let mut permission = daemon.current_permission("gh-authorized", "github/token");
    permission.writable = true;
    daemon.upsert_permission("test", &permission).await.unwrap();

    // Hand-crafted blob with a future schema_version. Must parse as
    // canonical JSON but be rejected before write.
    let bogus = br#"{"env":{},"files":{},"libsecret":{"collections":{}},"schema_version":999}"#;
    let resp = direct_finalize(
        &daemon,
        "gh-authorized",
        "github/token",
        Some(base64::engine::general_purpose::STANDARD.encode(bogus)),
    )
    .await;
    match resp {
        RuntimeWrapperInvocationFinalizeResponse::InvalidBlob { reason } => {
            assert_eq!(reason, "schema-unknown");
        }
        other => panic!("expected InvalidBlob(schema-unknown), got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn finalize_denies_with_no_permission_for_pair_without_grant() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    daemon.ensure_pair("gh-authorized", "github/token").unwrap();
    daemon
        .put_policy("test", &PolicyDocument::default())
        .await
        .unwrap();

    let resp = direct_finalize(&daemon, "gh-authorized", "github/token", None).await;
    match resp {
        RuntimeWrapperInvocationFinalizeResponse::Deny { reason } => {
            assert_eq!(reason, "no-permission");
        }
        other => panic!("expected Deny(no-permission), got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn finalize_denies_with_unregistered_pair_for_unknown_resource() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    let resp = direct_finalize(&daemon, "gh-authorized", "does/not/exist", None).await;
    match resp {
        RuntimeWrapperInvocationFinalizeResponse::Deny { reason } => {
            assert_eq!(reason, "unregistered-pair");
        }
        other => panic!("expected Deny(unregistered-pair), got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

// ===========================================================================
// Trust-boundary regressions
// ===========================================================================

/// The redesign deleted the per-key write path that addressed reserved
/// `control/...` storage keys directly. A runtime mTLS client must
/// have no shape it can put on the wire that reaches an internal key.
/// We try the obvious shapes — passing internal storage keys as
/// `resource_name` — and assert every one is rejected by the resource
/// lookup as "unregistered-pair", which is exactly the structured deny
/// the backend gives for any name not in the resources document.
#[tokio::test]
async fn runtime_client_cannot_address_control_internal_keys_via_resource_name() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    let attempted = [
        "control/policy",
        "control/resources",
        "control/pairs",
        "control/canary",
        "../control/policy",
        "../policy/document",
        // Legacy `<resource>/<slot>/<key>` shape — must also dead-end
        // at the same unregistered-pair gate.
        "github/token/env/GH_TOKEN",
    ];
    for name in attempted {
        let resp = direct_open(&daemon, "gh-authorized", name).await;
        match resp {
            RuntimeWrapperInvocationOpenResponse::Deny { reason } => {
                assert_eq!(
                    reason, "unregistered-pair",
                    "name {name:?} must dead-end at unregistered-pair, got reason {reason:?}",
                );
            }
            other => panic!("name {name:?} produced {other:?}, expected Deny"),
        }
    }
    daemon.stop().await.unwrap();
}

/// A runtime mTLS client must not be able to influence another client
/// instance's permissions by impersonating its `client_instance` in the
/// request. Identity comes from the cert, not the wire body, so even if
/// some other client instance has a writable permission on this exact
/// (wrapper, resource), this principal still gets `no-permission`.
#[tokio::test]
async fn open_uses_principal_from_cert_not_request_body() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_abc")
        .await
        .unwrap();
    // Grant the permission to a DIFFERENT client_instance only.
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                permissions: vec![WrapperPermission {
                    client_instance: "some-other-runtime".into(),
                    wrapper: "gh-authorized".into(),
                    resource_id: "github/token".into(),
                    mode: WrapperApprovalMode::Unlocked,
                    writable: true,
                    reason: None,
                }],
            },
        )
        .await
        .unwrap();

    // We connect with this daemon's cert (a different principal). The
    // permission must not be matched.
    match direct_open(&daemon, "gh-authorized", "github/token").await {
        RuntimeWrapperInvocationOpenResponse::Deny { reason } => {
            assert_eq!(reason, "no-permission");
        }
        other => panic!("expected Deny(no-permission), got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

/// The `PerCallApproval` mode was removed from `WrapperApprovalMode`.
/// Admin POSTs that try to upsert a permission carrying the legacy
/// variant must fail at deserialization (the variant is unknown to
/// serde now).
#[tokio::test]
async fn admin_policy_upsert_rejects_legacy_per_call_approval_mode_payload() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    // Build the raw JSON ourselves — we can't construct the variant
    // through the Rust type system because it no longer exists.
    let body = serde_json::json!({
        "mode": { "kind": "per-call-approval" },
        "writable": false,
        "reason": null,
    });
    let resp = daemon
        .admin_request(
            reqwest::Method::PUT,
            &format!(
                "/v1/admin/stores/test/wrapper-permissions/{}/gh-authorized/github%2Ftoken",
                daemon.client_instance(),
            ),
        )
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_client_error(),
        "legacy per-call-approval payload must be rejected, got {}",
        resp.status(),
    );
    daemon.stop().await.unwrap();
}
