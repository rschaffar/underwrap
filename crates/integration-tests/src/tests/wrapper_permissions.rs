use super::*;
use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};

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
