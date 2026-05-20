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
                permissions: vec![daemon.current_permission("gh-authorized", "github/token")],
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
    // Provenance must point at the calling client instance.
    let resource = resources
        .resources
        .iter()
        .find(|r| r.name == "github/token")
        .expect("auto-create on: resource github/token should appear");
    assert!(
        pairs
            .pairs
            .iter()
            .any(|p| p.wrapper == "gh-authorized" && p.resource_id == resource.id),
        "auto-create on: pair (gh-authorized, <github/token id>) should appear; got {pairs:?}",
    );
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

    let resource_record = daemon
        .create_resource("test", "github/token")
        .await
        .unwrap();
    let resource_id = resource_record.resource.id.clone();
    daemon
        .store_secret("test", "gh-authorized", &resource_id, "ghp_value")
        .await
        .unwrap();

    let blocked = daemon.delete_resource("test", &resource_id).await.unwrap();
    assert!(
        !blocked.ok,
        "delete should be refused while dependents exist"
    );
    let blockers = blocked.blocked_by.expect("blockers present");
    assert!(blockers.secret_count >= 1, "secret blocker reported");
    assert!(
        blockers.permission_count >= 1,
        "permission blocker reported"
    );

    // Clear the seeded permission.
    daemon
        .put_policy(
            "test",
            &underwrap_protocol::policy::PolicyDocument::default(),
        )
        .await
        .unwrap();

    // Remove the secret. The harness writes secrets under
    // `<resource_id>/env/TEST_TOKEN` (see `store_secret`).
    let resp = daemon
        .admin_request(reqwest::Method::POST, "/v1/admin/secret/delete")
        .json(&underwrap_protocol::service::AdminSecretDeleteRequest {
            store: "test".into(),
            resource_id: resource_id.clone(),
            slot: underwrap_protocol::resource::ResourceSlot::Env,
            key: "TEST_TOKEN".into(),
        })
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "secret delete must succeed: {}",
        resp.text().await.unwrap_or_default(),
    );

    let ok = daemon.delete_resource("test", &resource_id).await.unwrap();
    assert!(ok.ok, "delete should succeed after blockers cleared");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn service_mode_starts_unenrolled_when_backend_rejects_client_principal() {
    // A daemon presenting a client cert the backend doesn't recognise
    // still comes up — it logs a warning and binds wrapper sockets in
    // an unenrolled state so the operator can see and fix the
    // misconfiguration. Wrapper reads in this state must return
    // `Unauthorized` with the enrollment guidance, never a secret
    // value.
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start_service_mode(wrappers, "tunnel", "underwrap-local")
        .await
        .expect("daemon must start even when its credential is unrecognised");

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    match resp {
        WrapperResponse::Error { code, message } => {
            assert!(
                matches!(code, ErrorCode::Unauthorized),
                "expected Unauthorized, got {code:?}",
            );
            assert!(
                message.contains("not enrolled"),
                "expected enrollment guidance in message, got {message:?}",
            );
        }
        other => panic!("expected Error with Unauthorized, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}
