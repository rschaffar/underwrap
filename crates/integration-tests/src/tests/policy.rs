use super::*;
use underwrap_protocol::policy::{
    PolicyDocument, PolicyEffect, PolicyRule, WrapperApprovalMode, WrapperPolicyState,
};

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

    assert_eq!(
        daemon.get_policy("test").await.unwrap(),
        Some(PolicyDocument::default())
    );

    let document = PolicyDocument {
        wrappers: vec![WrapperPolicyState {
            wrapper: "gh-authorized".to_string(),
            mode: WrapperApprovalMode::Unlocked { until_epoch: None },
            reason: None,
        }],
        rules: vec![PolicyRule {
            id: "allow-read".to_string(),
            wrapper: "gh-authorized".to_string(),
            resource: "env:github/token".to_string(),
            operation: "env.read".to_string(),
            effect: PolicyEffect::Allow,
        }],
        grants: Vec::new(),
    };
    daemon.put_policy("test", &document).await.unwrap();

    let loaded = daemon.get_policy("test").await.unwrap().unwrap();
    assert_eq!(loaded, document);

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn wrapper_read_is_denied_when_policy_document_has_no_matching_rule() {
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
            assert!(message.contains("no backend policy rule"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn wrapper_read_succeeds_when_policy_document_allows_secret_read() {
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
                wrappers: Vec::new(),
                rules: vec![PolicyRule {
                    id: "allow-read".to_string(),
                    wrapper: "gh-authorized".to_string(),
                    resource: "env:github/token".to_string(),
                    operation: "env.read".to_string(),
                    effect: PolicyEffect::Allow,
                }],
                grants: Vec::new(),
            },
        )
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_test123");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn locked_wrapper_is_denied_even_with_allow_rule() {
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
                wrappers: vec![WrapperPolicyState {
                    wrapper: "gh-authorized".to_string(),
                    mode: WrapperApprovalMode::Locked,
                    reason: Some("locked centrally".to_string()),
                }],
                rules: vec![PolicyRule {
                    id: "allow-read".to_string(),
                    wrapper: "gh-authorized".to_string(),
                    resource: "env:github/token".to_string(),
                    operation: "env.read".to_string(),
                    effect: PolicyEffect::Allow,
                }],
                grants: Vec::new(),
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
