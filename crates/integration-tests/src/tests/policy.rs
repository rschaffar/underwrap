use super::*;
use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode};

#[tokio::test]
async fn admin_api_rejects_missing_or_invalid_session_cookie() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .add_root_certificate(
            reqwest::Certificate::from_pem(&std::fs::read(&daemon.service_ca_cert).unwrap())
                .unwrap(),
        )
        .build()
        .unwrap();

    let missing_cookie = client
        .get(format!("{}/v1/admin/stores", daemon.admin_url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        missing_cookie.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "expected 401 without session cookie"
    );

    let invalid_cookie = client
        .get(format!("{}/v1/admin/stores", daemon.admin_url))
        .header(reqwest::header::COOKIE, "__Host-uw-session=not-a-session")
        .send()
        .await
        .unwrap();
    assert_eq!(
        invalid_cookie.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "expected 401 with invalid session cookie"
    );

    let authenticated = daemon
        .admin_request(reqwest::Method::GET, "/v1/admin/stores")
        .send()
        .await
        .unwrap();
    assert!(
        authenticated.status().is_success(),
        "expected 200 with session auth"
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

    let grant = daemon.current_permission("gh-authorized", "github/token");
    let document = PolicyDocument {
        permissions: vec![grant.clone()],
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
            assert!(message.contains("no permission"));
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
                permissions: vec![daemon.current_permission("gh-authorized", "github/token")],
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
    let mut grant = daemon.current_permission("gh-authorized", "github/token");
    grant.mode = WrapperApprovalMode::Locked;
    grant.reason = Some("locked centrally".to_string());
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                permissions: vec![grant],
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
    let mut grant = daemon.current_permission("gh-authorized", "github/token");
    grant.mode = WrapperApprovalMode::UnlockedUntil { until_epoch: 1 };
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                permissions: vec![grant],
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
