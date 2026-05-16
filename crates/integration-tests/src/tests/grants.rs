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
        .upsert_grant("test", &daemon.current_user_grant("gh-authorized"))
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
    let grant = daemon.current_user_grant("gh-authorized");
    daemon.upsert_grant("test", &grant).await.unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_test123");

    daemon
        .delete_grant("test", &grant.wrapper, &grant.host, &grant.user)
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
async fn global_writes_toggle_gates_admin_secret_write() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();
    // Seed a secret so the policy document exists with writes_enabled = true.
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_initial")
        .await
        .unwrap();
    // Flip writes off — the next /v1/admin/secret/write must 403.
    // We call the raw admin write so the harness doesn't reset the
    // policy document on our behalf.
    daemon.set_writes_enabled("test", false).await.unwrap();
    let err = daemon
        .admin_write_raw("test", "gh-authorized", "github/token", "should-fail")
        .await
        .expect_err("write must fail when writes_enabled is off");
    assert!(
        err.to_string().contains("403"),
        "expected 403 from writes-disabled, got: {err}",
    );
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn ungranted_tuple_for_unrelated_user_still_rejects_test_runner() {
    // Regression: the daemon must lookup by `(wrapper, host, user)`,
    // not just `wrapper`. A grant for some other user on the same
    // wrapper doesn't grant the test runner.
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
                    wrapper: "gh-authorized".to_string(),
                    host: daemon.catalog_host().to_string(),
                    user: "not-the-test-runner".to_string(),
                    mode: WrapperApprovalMode::Unlocked,
                    reason: None,
                }],
                writes_enabled: true,
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
    let mut grant = daemon.current_user_grant("gh-authorized");
    grant.mode = WrapperApprovalMode::Locked;
    daemon.upsert_grant("test", &grant).await.unwrap();
    // Re-upsert with `Unlocked` — the daemon should now allow reads.
    grant.mode = WrapperApprovalMode::Unlocked;
    daemon.upsert_grant("test", &grant).await.unwrap();
    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_test123");
    daemon.stop().await.unwrap();
}
