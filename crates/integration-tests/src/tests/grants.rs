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
async fn admin_writes_are_unconditional_relative_to_per_resource_flags() {
    // Per-resource write flags only gate wrapper-side writes. Admin
    // writes through `/v1/admin/secret/write` and `/secret/delete`
    // succeed regardless of `files_writable` / `libsecret_writable`.
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_initial")
        .await
        .unwrap();
    // Explicitly clear (disable) the libsecret-writable flag for the
    // resource. The next admin write must still succeed.
    daemon
        .set_write_permission(
            "test",
            "github/token",
            underwrap_protocol::service::AdminWritableSlot::Libsecret,
            false,
        )
        .await
        .unwrap();
    daemon
        .admin_write_raw("test", "gh-authorized", "github/token", "ghp_rotated")
        .await
        .expect("admin writes are not gated by per-resource flags");
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn per_resource_write_permission_roundtrips_through_admin_api() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();
    // Seed a policy so the document exists; the flags default to
    // missing (denied).
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_initial")
        .await
        .unwrap();
    daemon
        .set_write_permission(
            "test",
            "github/token",
            underwrap_protocol::service::AdminWritableSlot::Files,
            true,
        )
        .await
        .unwrap();
    let doc = daemon
        .get_policy("test")
        .await
        .unwrap()
        .expect("policy document persists across reads");
    assert!(doc.files_writable_for("github/token"));
    assert!(!doc.libsecret_writable_for("github/token"));
    // Flip back off → key is removed from the map.
    daemon
        .set_write_permission(
            "test",
            "github/token",
            underwrap_protocol::service::AdminWritableSlot::Files,
            false,
        )
        .await
        .unwrap();
    let doc = daemon
        .get_policy("test")
        .await
        .unwrap()
        .expect("policy document still exists");
    assert!(!doc.files_writable_for("github/token"));
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
                ..Default::default()
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
