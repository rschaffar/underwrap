use super::*;
use base64::Engine;
use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
use underwrap_protocol::service::{ServiceDeleteRequest, ServiceWriteRequest};

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
            assert!(message.contains("no permission"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

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
            assert!(message.contains("no permission"));
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

    // Direct /v1/write — using the resource blob key, which is what
    // a daemon would address on writeback. Must be rejected.
    let write_resp = client
        .post(format!("{}/v1/write", daemon.service_url()))
        .json(&ServiceWriteRequest {
            secret_id: "resources/github/token".to_string(),
            material_b64: base64::engine::general_purpose::STANDARD.encode(b"{}"),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(
        write_resp.status(),
        reqwest::StatusCode::FORBIDDEN,
        "backend must reject /v1/write on a read-only permission regardless of what the client tries to send; got status {} body {}",
        write_resp.status(),
        write_resp.text().await.unwrap_or_default(),
    );

    // Direct /v1/delete — same threat model. Must also be rejected.
    let delete_resp = client
        .post(format!("{}/v1/delete", daemon.service_url()))
        .json(&ServiceDeleteRequest {
            secret_id: "resources/github/token".to_string(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(
        delete_resp.status(),
        reqwest::StatusCode::FORBIDDEN,
        "backend must reject /v1/delete on a read-only permission regardless of what the client tries to send; got status {} body {}",
        delete_resp.status(),
        delete_resp.text().await.unwrap_or_default(),
    );

    daemon.stop().await.unwrap();
}

/// Flip side of the above: when the permission IS writable, the same
/// direct call goes through. This guards against the test passing for
/// the wrong reason (e.g. an unrelated 403 from cert resolution or
/// route mis-handling).
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
    let write_resp = client
        .post(format!("{}/v1/write", daemon.service_url()))
        .json(&ServiceWriteRequest {
            secret_id: "resources/github/token".to_string(),
            material_b64: base64::engine::general_purpose::STANDARD.encode(b"{}"),
        })
        .send()
        .await
        .unwrap();
    assert!(
        write_resp.status().is_success(),
        "writable=true must let /v1/write through; got status {} body {}",
        write_resp.status(),
        write_resp.text().await.unwrap_or_default(),
    );

    daemon.stop().await.unwrap();
}
