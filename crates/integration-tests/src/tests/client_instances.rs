use super::*;
use underwrap_protocol::policy::PolicyDocument;
use underwrap_protocol::service::AdminClientInstanceStatus;

#[tokio::test]
async fn admin_api_lists_backend_managed_runtime_client_instances() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start_service_mode(wrappers, "local", "underwrap-local")
        .await
        .unwrap();

    let items = daemon.list_client_instances().await.unwrap().items;
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.display_name, "underwrap-local");
    assert_eq!(item.instance_name, "local");
    assert_eq!(item.current_store_id, "test");
    assert!(matches!(item.status, AdminClientInstanceStatus::Active));
    assert!(!item.current_credential_fingerprint.is_empty());

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn disabling_runtime_client_instance_blocks_fresh_runtime_connections() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start_service_mode(wrappers, "local", "underwrap-local")
        .await
        .unwrap();

    daemon
        .store_secret(
            "test",
            "gh-authorized",
            "github/token",
            "ghp_before_disable",
        )
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

    let first = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(first), "ghp_before_disable");
    let status = daemon.direct_service_status().await.unwrap();
    assert!(status.ok);

    let client_id = daemon.list_client_instances().await.unwrap().items[0]
        .id
        .clone();
    daemon
        .update_client_instance(
            &client_id,
            &underwrap_protocol::service::AdminClientInstanceUpdateRequest {
                display_name: None,
                hostname_hint: None,
                current_store_id: None,
                status: Some(AdminClientInstanceStatus::Disabled),
            },
        )
        .await
        .unwrap();

    let err = daemon
        .direct_service_status()
        .await
        .expect_err("disabled instances must fail on a fresh runtime connection");
    assert!(
        format!("{err:#}").contains("service API returned")
            || format!("{err:#}").contains("error sending request"),
        "unexpected error: {err:#}"
    );

    daemon.stop().await.unwrap();
}
