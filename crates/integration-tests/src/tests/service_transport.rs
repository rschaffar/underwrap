use super::*;
use underwrap_protocol::policy::PolicyDocument;

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
                grants: vec![daemon.current_grant("gh-authorized", "github/token")],
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
    assert!(
        entries
            .iter()
            .any(|e| e.client_instance == daemon.client_instance()
                && e.wrapper == "gh-authorized"
                && e.resource == "github/token")
    );

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn service_mode_rejects_unmapped_client_principal_at_startup() {
    // A daemon presenting a client cert the backend doesn't recognise
    // can't usefully serve anything — the backend authenticates every
    // request by mTLS fingerprint. The daemon refuses to start
    // (binding wrapper sockets) in this state so reads can never
    // accidentally succeed.
    let wrappers = one_wrapper_config();
    let result = TestDaemon::start_service_mode(wrappers, "tunnel", "underwrap-local").await;
    let err = match result {
        Ok(_) => panic!("daemon must refuse to start without a recognised client principal"),
        Err(e) => e,
    };
    let msg = format!("{err:#}");
    assert!(
        msg.contains("wrapper-gh-authorized.sock")
            || msg.contains("path never appeared")
            || msg.contains("client instance"),
        "unexpected startup error: {msg}",
    );
}
