use super::*;
use underwrap_protocol::policy::{PolicyDocument, PolicyEffect, PolicyRule};

#[tokio::test]
async fn service_mode_roundtrip_reads_secret() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start_service_mode(wrappers, "local", "underwrap-local")
        .await
        .unwrap();

    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_service")
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
    assert_eq!(expect_single_env_string(resp), "ghp_service");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn service_mode_registers_catalog_entries_on_startup() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start_service_mode(wrappers, "local", "underwrap-local")
        .await
        .unwrap();

    let entries = daemon.get_catalog("test").await.unwrap();
    assert!(entries.iter().any(|e| e.host == "local"
        && e.user == "*"
        && e.wrapper == "gh-authorized"
        && e.resource == "github/token"));

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn service_mode_rejects_unmapped_client_principal() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start_service_mode(wrappers, "tunnel", "underwrap-local")
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
