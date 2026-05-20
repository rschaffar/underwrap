use super::*;
use underwrap_protocol::policy::{AgentGrant, PolicyDocument, WrapperApprovalMode};

#[tokio::test]
async fn upsert_grant_allows_read_against_fresh_store() {
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
        .upsert_grant(
            "test",
            &daemon.current_grant("gh-authorized", "github/token"),
        )
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_test123");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn deleting_grant_revokes_future_reads() {
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
    let grant = daemon.current_grant("gh-authorized", "github/token");
    daemon.upsert_grant("test", &grant).await.unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_test123");

    daemon
        .delete_grant(
            "test",
            &grant.client_instance,
            &grant.wrapper,
            &grant.resource,
        )
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    match resp {
        WrapperResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::Unauthorized));
            assert!(message.contains("no grant"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn per_grant_writable_flag_roundtrips_through_admin_api() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_initial")
        .await
        .unwrap();
    let mut grant = daemon.current_grant("gh-authorized", "github/token");
    grant.writable = true;
    daemon.upsert_grant("test", &grant).await.unwrap();
    let doc = daemon
        .get_policy("test")
        .await
        .unwrap()
        .expect("policy document persists across reads");
    assert!(doc.writable_for(daemon.client_instance(), "gh-authorized", "github/token"));
    // Flip back off.
    grant.writable = false;
    daemon.upsert_grant("test", &grant).await.unwrap();
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
    // wrapper, resource)`. A grant for some other client instance on
    // the same wrapper does not grant this daemon.
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
                grants: vec![AgentGrant {
                    client_instance: "some-other-client".to_string(),
                    wrapper: "gh-authorized".to_string(),
                    resource: "github/token".to_string(),
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
            assert!(message.contains("no grant"));
        }
        other => panic!("expected Unauthorized, got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn upsert_replaces_mode_on_existing_grant_tuple() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_test123")
        .await
        .unwrap();
    let mut grant = daemon.current_grant("gh-authorized", "github/token");
    grant.mode = WrapperApprovalMode::Locked;
    daemon.upsert_grant("test", &grant).await.unwrap();
    // Re-upsert with `Unlocked` — the daemon should now allow reads.
    grant.mode = WrapperApprovalMode::Unlocked;
    daemon.upsert_grant("test", &grant).await.unwrap();
    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_test123");
    daemon.stop().await.unwrap();
}
