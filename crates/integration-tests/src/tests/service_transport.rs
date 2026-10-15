use base64::Engine;
use underwrap_backend_store::{
    PlainBackend, StorageBackend, load_resource_blob, save_resource_blob,
};
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::policy::PolicyDocument;
use underwrap_protocol::resource_blob::{Collection, Item, LibsecretSection, ResourceBlob};
use underwrap_protocol::service::{
    AdminSecretDeleteResponse, AdminSecretRenameRequest, AdminSecretWriteRequest,
};

use super::*;

fn mixed_resource_blob() -> ResourceBlob {
    let mut blob = ResourceBlob::empty();
    blob.env
        .insert("PRIMARY_TOKEN".into(), SecretBytes::from("primary-v1"));
    blob.env
        .insert("KEEP_TOKEN".into(), SecretBytes::from("keep-env"));
    blob.files
        .insert("primary.toml".into(), SecretBytes::from("primary = 1\n"));
    blob.files
        .insert("keep.toml".into(), SecretBytes::from("keep = true\n"));

    let mut attributes = std::collections::BTreeMap::new();
    attributes.insert("server".into(), "example.test".into());
    attributes.insert("account".into(), "operator".into());
    let item = Item {
        label: "CLI credential".into(),
        content_type: "application/octet-stream".into(),
        created: 101,
        modified: 202,
        attributes,
        secret: SecretBytes::from(vec![0, 1, 2, 0xff]),
    };
    let mut items = std::collections::BTreeMap::new();
    items.insert("cli-item".into(), item);
    let collection = Collection {
        label: "Default collection".into(),
        created: 11,
        modified: 22,
        items,
    };
    let mut collections = std::collections::BTreeMap::new();
    collections.insert("default".into(), collection);
    let mut aliases = std::collections::BTreeMap::new();
    aliases.insert("default".into(), "default".into());
    aliases.insert("login".into(), "default".into());
    blob.libsecret = LibsecretSection {
        aliases,
        collections,
    };
    blob
}

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

    let (_, blob, _) = checkout_resource(daemon.instance_socket.clone(), "gh-authorized").await;
    assert_eq!(expect_single_env_string(blob), "ghp_service");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn flat_secret_mutations_preserve_libsecret() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();
    let resource = daemon
        .create_resource("test", "mixed/resource")
        .await
        .unwrap()
        .resource;
    let store = PlainBackend::open(&daemon.store_dir_for("test")).unwrap();
    let mut expected = mixed_resource_blob();
    save_resource_blob(&store, &resource.id, &expected).unwrap();

    let response = daemon
        .admin_request(
            reqwest::Method::PUT,
            &format!(
                "/v1/admin/stores/test/secrets/{}/env/NEW_TOKEN",
                resource.id
            ),
        )
        .json(&AdminSecretWriteRequest {
            material_b64: base64::engine::general_purpose::STANDARD.encode(b"new-env"),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    expected
        .env
        .insert("NEW_TOKEN".into(), SecretBytes::from("new-env"));
    assert_eq!(load_resource_blob(&store, &resource.id).unwrap(), expected);

    let response = daemon
        .admin_request(
            reqwest::Method::PUT,
            &format!(
                "/v1/admin/stores/test/secrets/{}/files/primary.toml",
                resource.id
            ),
        )
        .json(&AdminSecretWriteRequest {
            material_b64: base64::engine::general_purpose::STANDARD.encode(b"primary = 2\n"),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    expected
        .files
        .insert("primary.toml".into(), SecretBytes::from("primary = 2\n"));
    assert_eq!(load_resource_blob(&store, &resource.id).unwrap(), expected);

    let response = daemon
        .admin_request(
            reqwest::Method::DELETE,
            &format!(
                "/v1/admin/stores/test/secrets/{}/files/keep.toml",
                resource.id
            ),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let deleted: AdminSecretDeleteResponse = response.json().await.unwrap();
    assert!(deleted.existed);
    expected.files.remove("keep.toml");
    assert_eq!(load_resource_blob(&store, &resource.id).unwrap(), expected);

    let response = daemon
        .admin_request(
            reqwest::Method::POST,
            &format!(
                "/v1/admin/stores/test/secrets/{}/env/PRIMARY_TOKEN/rename",
                resource.id
            ),
        )
        .json(&AdminSecretRenameRequest {
            to_key: "RENAMED_TOKEN".into(),
            material_b64: None,
        })
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let material = expected.env.remove("PRIMARY_TOKEN").unwrap();
    expected.env.insert("RENAMED_TOKEN".into(), material);
    assert_eq!(load_resource_blob(&store, &resource.id).unwrap(), expected);

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
async fn admin_resource_delete_reports_blockers_then_removes_clean_resources() {
    let daemon = TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local")
        .await
        .unwrap();

    // First exercise env content together with pair and permission blockers,
    // then clear every dependency and delete the resource successfully.
    let resource_id = daemon
        .create_resource("test", "github/token")
        .await
        .unwrap()
        .resource
        .id;
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
    assert!(
        blockers.content_entry_count >= 1,
        "content blocker reported"
    );
    assert!(blockers.pair_count >= 1, "pair blocker reported");
    assert!(
        blockers.permission_count >= 1,
        "permission blocker reported"
    );

    daemon
        .put_policy("test", &PolicyDocument::default())
        .await
        .unwrap();
    let response = daemon
        .admin_request(
            reqwest::Method::DELETE,
            &format!("/v1/admin/stores/test/secrets/{resource_id}/env/TEST_TOKEN"),
        )
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "secret delete must succeed: {}",
        response.text().await.unwrap_or_default(),
    );
    daemon
        .delete_pair("test", "gh-authorized", &resource_id)
        .await
        .unwrap();

    let deleted = daemon.delete_resource("test", &resource_id).await.unwrap();
    assert!(deleted.ok, "delete should succeed after blockers clear");

    // Then create a separate resource entirely through the structured API,
    // verify its exact libsecret blockers preserve catalog and blob state,
    // clean it through that API, and confirm its active storage key is gone.
    let resource = daemon
        .create_resource("test", "libsecret/delete-flow")
        .await
        .unwrap()
        .resource;
    let libsecret_path = format!("/v1/admin/stores/test/resources/{}/libsecret", resource.id);

    let response = daemon
        .admin_request(
            reqwest::Method::POST,
            &format!("{libsecret_path}/collections"),
        )
        .json(&serde_json::json!({"id": "default", "label": "Default"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let response = daemon
        .admin_request(
            reqwest::Method::POST,
            &format!("{libsecret_path}/collections/default/items"),
        )
        .json(&serde_json::json!({
            "id": "0123456789abcdef0123456789abcdef",
            "label": "credential",
            "content_type": "text/plain",
            "attributes": {"service": "example"},
            "secret_b64": base64::engine::general_purpose::STANDARD.encode(b"test-value"),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let response = daemon
        .admin_request(
            reqwest::Method::PUT,
            &format!("{libsecret_path}/aliases/default"),
        )
        .json(&serde_json::json!({"collection_id": "default"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let catalog_before = daemon.list_resources("test").await.unwrap();
    let blob_before = daemon.get_resource_blob("test", &resource.id).unwrap();
    let blocked = daemon.delete_resource("test", &resource.id).await.unwrap();
    assert!(!blocked.ok);
    let blockers = blocked.blocked_by.expect("typed blockers");
    assert_eq!(blockers.content_entry_count, 3);
    assert_eq!(
        serde_json::to_value(&blockers.sample_content).unwrap(),
        serde_json::json!([
            {"kind": "libsecret_alias", "name": "default"},
            {"kind": "libsecret_collection", "collection_id": "default"},
            {
                "kind": "libsecret_item",
                "collection_id": "default",
                "item_id": "0123456789abcdef0123456789abcdef"
            }
        ])
    );
    assert_eq!(daemon.list_resources("test").await.unwrap(), catalog_before);
    assert_eq!(
        daemon.get_resource_blob("test", &resource.id).unwrap(),
        blob_before
    );

    for path in [
        format!("{libsecret_path}/aliases/default"),
        format!("{libsecret_path}/collections/default/items/0123456789abcdef0123456789abcdef"),
        format!("{libsecret_path}/collections/default"),
    ] {
        let response = daemon
            .admin_request(reqwest::Method::DELETE, &path)
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "libsecret cleanup failed for {path}: {}",
            response.text().await.unwrap_or_default()
        );
    }
    assert_eq!(
        daemon.get_resource_blob("test", &resource.id).unwrap(),
        ResourceBlob::empty()
    );

    let deleted = daemon.delete_resource("test", &resource.id).await.unwrap();
    assert!(deleted.ok);
    let store = PlainBackend::open(&daemon.store_dir_for("test")).unwrap();
    let active_key = format!("resources/{}", resource.id);
    assert_eq!(store.read(&active_key).unwrap(), None);
    assert!(!store.list("resources").unwrap().contains(&active_key));
    assert!(
        daemon
            .list_resources("test")
            .await
            .unwrap()
            .resources
            .iter()
            .all(|candidate| candidate.id != resource.id)
    );

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn flat_secret_endpoints_reject_libsecret_slot() {
    // Libsecret content is structured (Service → Collections → Items
    // + an alias table) and only addressable through the dedicated
    // /v1/admin/stores/{sid}/resources/{rid}/libsecret/* endpoints.
    // The flat /secrets/{rid}/{slot}/{key} endpoints stay for env /
    // files only and must surface `slot_not_supported` for libsecret
    // attempts so stale clients get a clear pointer at the new
    // surface.
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start_service_mode(wrappers, "local", "underwrap-local")
        .await
        .unwrap();

    let resource_record = daemon
        .create_resource("test", "libsecret/demo")
        .await
        .unwrap();
    let resource_id = resource_record.resource.id.clone();

    // Every flat-secret verb (GET / PUT / DELETE) must produce the
    // same `slot_not_supported` rejection. PUT carries a body, so
    // include one to prove the slot check fires before body parsing.
    let cases: Vec<(reqwest::Method, Option<serde_json::Value>)> = vec![
        (reqwest::Method::GET, None),
        (
            reqwest::Method::PUT,
            Some(serde_json::json!({ "material_b64": "eA==" })),
        ),
        (reqwest::Method::DELETE, None),
    ];
    for (method, body) in cases {
        let mut req = daemon.admin_request(
            method.clone(),
            &format!("/v1/admin/stores/test/secrets/{resource_id}/libsecret/_aliases"),
        );
        if let Some(json) = body {
            req = req.json(&json);
        }
        let resp = req.send().await.unwrap();
        assert_eq!(
            resp.status(),
            reqwest::StatusCode::BAD_REQUEST,
            "{method} on flat libsecret slot must be rejected with 400",
        );
        let envelope: underwrap_protocol::service::ServiceErrorResponse =
            resp.json().await.unwrap();
        assert_eq!(envelope.code, "slot_not_supported");
        assert!(
            envelope.message.contains("/libsecret/"),
            "error message should point at the structured endpoint, got {envelope:?}",
        );
    }

    // The structured tree endpoint still works for the same resource.
    let resp = daemon
        .admin_request(
            reqwest::Method::GET,
            &format!("/v1/admin/stores/test/resources/{resource_id}/libsecret"),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

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
