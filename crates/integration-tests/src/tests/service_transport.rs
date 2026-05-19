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
async fn service_mode_refuses_auto_create_when_disabled() {
    // Fixture defaults: client instance has auto_create_wrapper_assets
    // off. The daemon's startup register call must not create resource
    // or pair records; the backend responds with AutoCreateDisabled,
    // and the documents stay empty.
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start_service_mode(wrappers, "local", "underwrap-local")
        .await
        .unwrap();

    let resources = daemon.list_resources("test").await.unwrap();
    let pairs = daemon.list_pairs("test").await.unwrap();
    assert!(
        resources.resources.is_empty(),
        "auto-create off: no resources should be created, got {resources:?}",
    );
    assert!(
        pairs.pairs.is_empty(),
        "auto-create off: no pairs should be created, got {pairs:?}",
    );

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn service_mode_auto_creates_wrapper_assets_when_enabled() {
    // With auto-create on at daemon startup, the register call creates
    // the resource and pair records with provenance pointing at the
    // calling client instance.
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start_service_mode_with(wrappers, "local", "underwrap-local", true)
        .await
        .unwrap();

    let instance_id = daemon.client_instance().to_string();

    let resources = daemon.list_resources("test").await.unwrap();
    let pairs = daemon.list_pairs("test").await.unwrap();
    assert!(
        resources.resources.iter().any(|r| r.name == "github/token"),
        "auto-create on: resource github/token should appear; got {resources:?}",
    );
    assert!(
        pairs
            .pairs
            .iter()
            .any(|p| p.wrapper == "gh-authorized" && p.resource == "github/token"),
        "auto-create on: pair (gh-authorized, github/token) should appear; got {pairs:?}",
    );
    // Provenance must point at the calling client instance.
    let resource = resources
        .resources
        .iter()
        .find(|r| r.name == "github/token")
        .unwrap();
    match &resource.created_by {
        underwrap_protocol::entities::EntityCreator::ClientInstance { id } => {
            assert_eq!(id, &instance_id);
        }
        other => panic!("expected ClientInstance creator, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn admin_resource_delete_refuses_when_secrets_still_present() {
    // Admin creates a resource, writes a secret under it (which the
    // harness helper also seeds a grant for), attempts to delete:
    // backend responds with structured blockers. After both the
    // secret and the grant are cleared, delete succeeds.
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start_service_mode(wrappers, "local", "underwrap-local")
        .await
        .unwrap();

    daemon
        .create_resource("test", "github/token")
        .await
        .unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_value")
        .await
        .unwrap();

    let blocked = daemon
        .delete_resource("test", "github/token")
        .await
        .unwrap();
    assert!(
        !blocked.ok,
        "delete should be refused while dependents exist"
    );
    let blockers = blocked.blocked_by.expect("blockers present");
    assert!(blockers.secret_count >= 1, "secret blocker reported");
    assert!(blockers.grant_count >= 1, "grant blocker reported");

    // Clear the seeded grant.
    daemon
        .put_policy(
            "test",
            &underwrap_protocol::policy::PolicyDocument::default(),
        )
        .await
        .unwrap();

    // Remove the secret. The harness writes secrets under
    // `<resource>/env/TEST_TOKEN` (see `env_secret_id`).
    let resp = daemon
        .admin_request(reqwest::Method::POST, "/v1/admin/secret/delete")
        .json(&underwrap_protocol::service::AdminSecretDeleteRequest {
            store: "test".into(),
            secret_id: "github/token/env/TEST_TOKEN".into(),
        })
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "secret delete must succeed: {}",
        resp.text().await.unwrap_or_default(),
    );

    let ok = daemon
        .delete_resource("test", "github/token")
        .await
        .unwrap();
    assert!(ok.ok, "delete should succeed after blockers cleared");

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
