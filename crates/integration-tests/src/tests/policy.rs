use super::*;
use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode};

#[tokio::test]
async fn admin_api_rejects_missing_or_wrong_bearer_token() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    let client = reqwest::Client::new();

    let no_token = client
        .get(format!("{}/v1/admin/stores", daemon.admin_url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        no_token.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "expected 401 without bearer token"
    );

    let wrong_token = client
        .get(format!("{}/v1/admin/stores", daemon.admin_url))
        .bearer_auth("nope-not-the-token")
        .send()
        .await
        .unwrap();
    assert_eq!(
        wrong_token.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "expected 401 with wrong bearer token"
    );

    let right_token = client
        .get(format!("{}/v1/admin/stores", daemon.admin_url))
        .bearer_auth(&daemon.admin_token)
        .send()
        .await
        .unwrap();
    assert!(
        right_token.status().is_success(),
        "expected 200 with right token"
    );

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn backend_policy_roundtrip_over_admin_api() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    // Default policy on a fresh store: no grants, writes disabled.
    assert_eq!(
        daemon.get_policy("test").await.unwrap(),
        Some(PolicyDocument::default())
    );

    let grant = daemon.current_user_grant("gh-authorized");
    let document = PolicyDocument {
        grants: vec![grant.clone()],
        ..Default::default()
    };
    daemon.put_policy("test", &document).await.unwrap();

    let loaded = daemon.get_policy("test").await.unwrap().unwrap();
    assert_eq!(loaded, document);

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn wrapper_read_is_denied_when_no_grant_for_principal() {
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
async fn wrapper_read_succeeds_when_grant_is_unlocked() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_test123")
        .await
        .unwrap();
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                grants: vec![daemon.current_user_grant("gh-authorized")],
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_test123");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn locked_grant_denies_read() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_test123")
        .await
        .unwrap();
    let mut grant = daemon.current_user_grant("gh-authorized");
    grant.mode = WrapperApprovalMode::Locked;
    grant.reason = Some("locked centrally".to_string());
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                grants: vec![grant],
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    match resp {
        WrapperResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::Unauthorized));
            assert!(message.contains("locked centrally"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn unlocked_until_grant_denies_after_deadline() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_test123")
        .await
        .unwrap();
    // Stamp a deadline far in the past so the grant is already
    // expired by the time we read. The expiry check is lazy on
    // request, so there's no flaky sleep involved.
    let mut grant = daemon.current_user_grant("gh-authorized");
    grant.mode = WrapperApprovalMode::UnlockedUntil { until_epoch: 1 };
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                grants: vec![grant],
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let resp = daemon.read_request("gh-authorized").await.unwrap();
    match resp {
        WrapperResponse::Error { code, .. } => {
            assert!(matches!(code, ErrorCode::Unauthorized));
        }
        other => panic!("expected Unauthorized after deadline, got {other:?}"),
    }
    daemon.stop().await.unwrap();
}
