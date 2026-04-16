use super::*;
use underwrap_protocol::policy::{PolicyDocument, PolicyEffect, PolicyRule};

#[tokio::test]
async fn service_mode_roundtrip_reads_secret() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start_service_mode(wrappers, secrets, "local", "underwrap-local")
        .await
        .unwrap();

    daemon
        .store_secret("test", "github/token", "ghp_service")
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
                    resource: "secret:github/token".to_string(),
                    operation: "secret.read".to_string(),
                    effect: PolicyEffect::Allow,
                }],
            },
        )
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_service");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn service_mode_rejects_unmapped_client_principal() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start_service_mode(wrappers, secrets, "tunnel", "underwrap-local")
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    match resp {
        WrapperResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::Internal));
            assert!(
                message.contains("internal storage error")
                    || message.contains("internal policy error")
            );
        }
        other => panic!("expected Error, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}
