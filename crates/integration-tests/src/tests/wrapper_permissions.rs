use super::*;
use base64::Engine;
use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
use underwrap_protocol::resource_blob::ResourceBlob;
use underwrap_protocol::service::{
    RuntimeWrapperInvocationFinalizeRequest, RuntimeWrapperInvocationFinalizeResponse,
    RuntimeWrapperInvocationOpenRequest, RuntimeWrapperInvocationOpenResponse,
};
use underwrap_protocol::variant::WrapperVariant;

#[tokio::test]
async fn upsert_permission_allows_read_against_fresh_store() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_test123")
        .await
        .unwrap();
    daemon
        .put_policy("test", &PolicyDocument::default())
        .await
        .unwrap();

    daemon
        .upsert_permission(
            "test",
            &daemon.current_permission("gh-authorized", "github/token"),
        )
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_test123");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn deleting_permission_revokes_future_reads() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_test123")
        .await
        .unwrap();
    daemon
        .put_policy("test", &PolicyDocument::default())
        .await
        .unwrap();
    let permission = daemon.current_permission("gh-authorized", "github/token");
    daemon.upsert_permission("test", &permission).await.unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_test123");

    daemon
        .delete_permission(
            "test",
            &permission.client_instance,
            &permission.wrapper,
            &permission.resource_id,
        )
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    match resp {
        WrapperResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::Unauthorized));
            assert!(message.contains("no-permission"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn debug_variant_denied_under_unlocked_allowed_under_unsafe_debug() {
    // End-to-end coverage of the (mode × variant) matrix over the real
    // daemon→backend mTLS path — the wire path the VM test exercises and
    // that unit tests of `decide` do not cover.
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_test123")
        .await
        .unwrap();
    daemon
        .put_policy("test", &PolicyDocument::default())
        .await
        .unwrap();

    // Unlocked permission: Real is allowed, Debug must be denied.
    let permission = daemon.current_permission("gh-authorized", "github/token");
    assert!(matches!(permission.mode, WrapperApprovalMode::Unlocked));
    daemon.upsert_permission("test", &permission).await.unwrap();

    let real = daemon
        .read_request_variant("gh-authorized", WrapperVariant::Real)
        .await
        .unwrap();
    assert!(
        matches!(real, WrapperResponse::ReadOk { .. }),
        "Real under Unlocked must be allowed, got {real:?}"
    );

    let debug = daemon
        .read_request_variant("gh-authorized", WrapperVariant::Debug)
        .await
        .unwrap();
    match debug {
        WrapperResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::Unauthorized));
            assert!(
                message.contains("not-unsafe-debug"),
                "expected not-unsafe-debug deny, got: {message}"
            );
        }
        other => panic!("Debug under Unlocked must be denied, got {other:?}"),
    }

    // UnsafeDebugUntil (future): both variants allowed.
    let mut unsafe_debug = daemon.current_permission("gh-authorized", "github/token");
    unsafe_debug.mode = WrapperApprovalMode::UnsafeDebugUntil {
        until_epoch: u64::MAX,
    };
    daemon
        .upsert_permission("test", &unsafe_debug)
        .await
        .unwrap();

    let debug_ok = daemon
        .read_request_variant("gh-authorized", WrapperVariant::Debug)
        .await
        .unwrap();
    assert!(
        matches!(debug_ok, WrapperResponse::ReadOk { .. }),
        "Debug under UnsafeDebugUntil must be allowed, got {debug_ok:?}"
    );

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn per_permission_writable_flag_roundtrips_through_admin_api() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_initial")
        .await
        .unwrap();
    let mut permission = daemon.current_permission("gh-authorized", "github/token");
    permission.writable = true;
    daemon.upsert_permission("test", &permission).await.unwrap();
    let doc = daemon
        .get_policy("test")
        .await
        .unwrap()
        .expect("policy document persists across reads");
    assert!(doc.writable_for(daemon.client_instance(), "gh-authorized", "github/token"));
    // Flip back off.
    permission.writable = false;
    daemon.upsert_permission("test", &permission).await.unwrap();
    let doc = daemon
        .get_policy("test")
        .await
        .unwrap()
        .expect("policy document still exists");
    assert!(!doc.writable_for(daemon.client_instance(), "gh-authorized", "github/token"));
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn ungranted_tuple_for_unrelated_client_instance_still_rejects() {
    // Regression: the daemon must look up by `(client_instance,
    // wrapper, resource)`. A permission for some other client instance
    // on the same wrapper does not grant this daemon.
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_test")
        .await
        .unwrap();
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                permissions: vec![WrapperPermission {
                    client_instance: "some-other-client".to_string(),
                    wrapper: "gh-authorized".to_string(),
                    resource_id: "github/token".to_string(),
                    mode: WrapperApprovalMode::Unlocked,
                    writable: false,
                    reason: None,
                }],
            },
        )
        .await
        .unwrap();
    let resp = daemon.read_request("gh-authorized").await.unwrap();
    match resp {
        WrapperResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::Unauthorized));
            assert!(message.contains("no-permission"));
        }
        other => panic!("expected Unauthorized, got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn upsert_replaces_mode_on_existing_permission_tuple() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_test123")
        .await
        .unwrap();
    let mut permission = daemon.current_permission("gh-authorized", "github/token");
    permission.mode = WrapperApprovalMode::Locked;
    daemon.upsert_permission("test", &permission).await.unwrap();
    // Re-upsert with `Unlocked` — the daemon should now allow reads.
    permission.mode = WrapperApprovalMode::Unlocked;
    daemon.upsert_permission("test", &permission).await.unwrap();
    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_test123");
    daemon.stop().await.unwrap();
}

/// Trust-boundary regression: the backend MUST enforce the `writable`
/// flag itself, not rely on the frontend daemon honouring it. We
/// bypass the daemon entirely and hit `/v1/write` and `/v1/delete`
/// over mTLS as if a malicious or buggy client tried to escalate a
/// read-only grant to a write. Both endpoints must answer 403.
#[tokio::test]
async fn backend_rejects_writes_on_readonly_permission_when_daemon_is_bypassed() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start_service_mode(wrappers, "local", "underwrap-local")
        .await
        .unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_initial")
        .await
        .unwrap();
    // writable=false on purpose — the gate must reject /v1/write and
    // /v1/delete even though the cert identifies a granted principal.
    let mut permission = daemon.current_permission("gh-authorized", "github/token");
    permission.writable = false;
    daemon.upsert_permission("test", &permission).await.unwrap();

    let client = daemon.direct_service_client().unwrap();
    let blob = ResourceBlob::empty().to_canonical_bytes().unwrap();

    // Direct finalize attempt against the new endpoint. The backend
    // must reject because writable=false, regardless of what the
    // client tries to send.
    let resp = client
        .post(format!(
            "{}/v1/wrapper-invocations/finalize",
            daemon.service_url()
        ))
        .json(&RuntimeWrapperInvocationFinalizeRequest {
            wrapper_name: "gh-authorized".to_string(),
            resource_name: "github/token".to_string(),
            variant: WrapperVariant::Real,
            blob_b64: Some(base64::engine::general_purpose::STANDARD.encode(&blob)),
        })
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "backend should answer 200 with a structured Deny; got status {}",
        resp.status(),
    );
    let parsed: RuntimeWrapperInvocationFinalizeResponse = resp.json().await.unwrap();
    match parsed {
        RuntimeWrapperInvocationFinalizeResponse::Deny { reason } => {
            assert_eq!(reason, "not-writable");
        }
        other => panic!("expected Deny(not-writable), got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

/// Flip side of the above: when the permission IS writable, the same
/// direct finalize goes through. This guards against the test passing
/// for the wrong reason (e.g. an unrelated 403 from cert resolution
/// or route mis-handling).
#[tokio::test]
async fn backend_allows_writes_on_writable_permission_via_direct_call() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start_service_mode(wrappers, "local", "underwrap-local")
        .await
        .unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_initial")
        .await
        .unwrap();
    let mut permission = daemon.current_permission("gh-authorized", "github/token");
    permission.writable = true;
    daemon.upsert_permission("test", &permission).await.unwrap();

    let client = daemon.direct_service_client().unwrap();
    // Fetch the current canonical blob via open so we don't trip
    // env-section-immutability on the finalize attempt.
    let open_resp = client
        .post(format!("{}/v1/wrapper-invocations", daemon.service_url()))
        .json(&RuntimeWrapperInvocationOpenRequest {
            wrapper_name: "gh-authorized".to_string(),
            resource_name: "github/token".to_string(),
            variant: WrapperVariant::Real,
        })
        .send()
        .await
        .unwrap();
    let current_blob_b64 = match open_resp
        .json::<RuntimeWrapperInvocationOpenResponse>()
        .await
        .unwrap()
    {
        RuntimeWrapperInvocationOpenResponse::Allow { blob_b64, .. } => blob_b64,
        other => panic!("expected Allow on open, got {other:?}"),
    };

    let resp = client
        .post(format!(
            "{}/v1/wrapper-invocations/finalize",
            daemon.service_url()
        ))
        .json(&RuntimeWrapperInvocationFinalizeRequest {
            wrapper_name: "gh-authorized".to_string(),
            resource_name: "github/token".to_string(),
            variant: WrapperVariant::Real,
            blob_b64: Some(current_blob_b64),
        })
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let parsed: RuntimeWrapperInvocationFinalizeResponse = resp.json().await.unwrap();
    assert!(
        matches!(parsed, RuntimeWrapperInvocationFinalizeResponse::Ok),
        "writable=true must let finalize through; got {parsed:?}",
    );

    daemon.stop().await.unwrap();
}
