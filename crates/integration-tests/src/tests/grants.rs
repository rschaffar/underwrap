use super::*;
use underwrap_protocol::policy::{PolicyDocument, PolicyEffect, PolicyGrant};

#[tokio::test]
async fn active_allow_grant_allows_read_without_baseline_rule() {
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

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    daemon
        .upsert_grant(
            "test",
            &PolicyGrant {
                id: "g-allow".to_string(),
                wrapper: "gh-authorized".to_string(),
                resource: "env:github/token".to_string(),
                operation: "env.read".to_string(),
                effect: PolicyEffect::Allow,
                starts_at_epoch: Some(now.saturating_sub(1)),
                expires_at_epoch: Some(now + 60),
                reason: Some("temporary access".to_string()),
            },
        )
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_test123");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn expired_grant_is_not_honored() {
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

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    daemon
        .upsert_grant(
            "test",
            &PolicyGrant {
                id: "g-expired".to_string(),
                wrapper: "gh-authorized".to_string(),
                resource: "env:github/token".to_string(),
                operation: "env.read".to_string(),
                effect: PolicyEffect::Allow,
                starts_at_epoch: Some(now.saturating_sub(120)),
                expires_at_epoch: Some(now.saturating_sub(60)),
                reason: Some("expired access".to_string()),
            },
        )
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    match resp {
        WrapperResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::Unauthorized));
            assert!(message.contains("no backend policy rule or active grant"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

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

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    daemon
        .upsert_grant(
            "test",
            &PolicyGrant {
                id: "g-delete".to_string(),
                wrapper: "gh-authorized".to_string(),
                resource: "env:github/token".to_string(),
                operation: "env.read".to_string(),
                effect: PolicyEffect::Allow,
                starts_at_epoch: Some(now.saturating_sub(1)),
                expires_at_epoch: Some(now + 60),
                reason: Some("temporary access".to_string()),
            },
        )
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_test123");

    daemon.delete_grant("test", "g-delete").await.unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    match resp {
        WrapperResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::Unauthorized));
            assert!(message.contains("no backend policy rule or active grant"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}
