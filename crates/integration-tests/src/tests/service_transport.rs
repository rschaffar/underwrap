use super::*;
use base64::Engine;
use std::collections::BTreeMap;
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::policy::PolicyDocument;
use underwrap_protocol::resource_blob::{Collection, Item, LibsecretSection, ResourceBlob};

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

    // Delete the (wrapper, resource) pair record too — the new test
    // harness seeds one inside `store_secret` so the open-invocation
    // gate can find it, and the resource delete refuses while any
    // pair references the resource.
    daemon
        .delete_pair("test", "gh-authorized", &resource_id)
        .await
        .unwrap();

    let ok = daemon.delete_resource("test", &resource_id).await.unwrap();
    assert!(ok.ok, "delete should succeed after blockers cleared");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn admin_secret_delete_removes_single_libsecret_entry() {
    // /v1/admin/secret/delete is slot-agnostic on the surface but the
    // libsecret slot round-trips through a richer canonical form
    // (`_aliases` + `<col>/_meta` + `<col>/<item>.meta` +
    // `<col>/<item>.secret`) than env/files. This test seeds a blob
    // with two libsecret items and asserts that deleting one item's
    // `.secret` removes exactly that one item (the orphan-meta scrub
    // in `libsecret_section_from_kv` drops the matching `.meta` on
    // round-trip), while the second item and the rest of the section
    // survive untouched.
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start_service_mode(wrappers, "local", "underwrap-local")
        .await
        .unwrap();

    let resource_record = daemon
        .create_resource("test", "libsecret/demo")
        .await
        .unwrap();
    let resource_id = resource_record.resource.id.clone();

    // Hand-build a libsecret section with one collection holding two
    // items so the round-trip after the targeted delete leaves the
    // other item intact.
    let mut items = BTreeMap::new();
    items.insert(
        "keep".into(),
        Item {
            label: "Keep".into(),
            content_type: "text/plain".into(),
            created: 1_000,
            modified: 1_000,
            attributes: BTreeMap::new(),
            secret: SecretBytes::from(b"keep_value".to_vec()),
        },
    );
    items.insert(
        "drop".into(),
        Item {
            label: "Drop".into(),
            content_type: "text/plain".into(),
            created: 2_000,
            modified: 2_000,
            attributes: BTreeMap::new(),
            secret: SecretBytes::from(b"drop_value".to_vec()),
        },
    );
    let mut collections = BTreeMap::new();
    collections.insert(
        "default".into(),
        Collection {
            label: "Default".into(),
            created: 1_000,
            modified: 1_000,
            items,
        },
    );
    let mut aliases = BTreeMap::new();
    aliases.insert("default".into(), "default".into());
    let mut blob = ResourceBlob::empty();
    blob.libsecret = LibsecretSection {
        aliases,
        collections,
    };
    let blob_b64 =
        base64::engine::general_purpose::STANDARD.encode(blob.to_canonical_bytes().unwrap());
    let put = daemon
        .put_resource_blob("test", &resource_id, &blob_b64)
        .await
        .unwrap();
    assert!(
        put.status().is_success(),
        "seed PUT must succeed: {}",
        put.status()
    );

    // Sanity: both items' .secret entries are visible through the
    // admin list before the delete.
    let listed = daemon
        .admin_request(reqwest::Method::POST, "/v1/admin/secret/list")
        .json(&underwrap_protocol::service::AdminSecretListRequest {
            store: "test".into(),
            resource_id: Some(resource_id.clone()),
        })
        .send()
        .await
        .unwrap()
        .json::<underwrap_protocol::service::AdminSecretListResponse>()
        .await
        .unwrap();
    let libsecret_keys: Vec<String> = listed
        .entries
        .iter()
        .filter(|e| {
            matches!(
                e.slot,
                underwrap_protocol::resource::ResourceSlot::Libsecret
            )
        })
        .map(|e| e.key.clone())
        .collect();
    assert!(
        libsecret_keys.contains(&"default/drop.secret".to_string()),
        "expected drop.secret in libsecret listing, got {libsecret_keys:?}",
    );
    assert!(
        libsecret_keys.contains(&"default/keep.secret".to_string()),
        "expected keep.secret in libsecret listing, got {libsecret_keys:?}",
    );

    // Delete just the dropped item's `.secret` entry.
    let resp = daemon
        .admin_request(reqwest::Method::POST, "/v1/admin/secret/delete")
        .json(&underwrap_protocol::service::AdminSecretDeleteRequest {
            store: "test".into(),
            resource_id: resource_id.clone(),
            slot: underwrap_protocol::resource::ResourceSlot::Libsecret,
            key: "default/drop.secret".into(),
        })
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "libsecret secret-delete must succeed: {}",
        resp.status(),
    );
    let body: underwrap_protocol::service::AdminSecretDeleteResponse = resp.json().await.unwrap();
    assert!(
        body.existed,
        "secret-delete must report existed=true for a seeded entry",
    );

    // The "drop" item is gone and the "keep" item survives. The
    // section's aliases and collection meta also survive because
    // they were emitted as separate blob entries.
    let after = daemon
        .get_resource_blob("test", &resource_id)
        .await
        .unwrap();
    let after_bytes = base64::engine::general_purpose::STANDARD
        .decode(&after.blob_b64)
        .unwrap();
    let after_blob = ResourceBlob::from_canonical_bytes(&after_bytes).unwrap();
    let default_col = after_blob
        .libsecret
        .collections
        .get("default")
        .expect("default collection survives");
    assert!(
        default_col.items.contains_key("keep"),
        "kept item must remain after libsecret entry delete, got items {:?}",
        default_col.items.keys().collect::<Vec<_>>(),
    );
    assert!(
        !default_col.items.contains_key("drop"),
        "dropped item must be gone after libsecret entry delete",
    );
    assert_eq!(
        after_blob
            .libsecret
            .aliases
            .get("default")
            .map(String::as_str),
        Some("default"),
        "aliases must survive",
    );

    // Listing again reflects the change: only `keep.secret` (and
    // friends) remain on the libsecret slot.
    let listed_after = daemon
        .admin_request(reqwest::Method::POST, "/v1/admin/secret/list")
        .json(&underwrap_protocol::service::AdminSecretListRequest {
            store: "test".into(),
            resource_id: Some(resource_id.clone()),
        })
        .send()
        .await
        .unwrap()
        .json::<underwrap_protocol::service::AdminSecretListResponse>()
        .await
        .unwrap();
    let libsecret_keys_after: Vec<String> = listed_after
        .entries
        .iter()
        .filter(|e| {
            matches!(
                e.slot,
                underwrap_protocol::resource::ResourceSlot::Libsecret
            )
        })
        .map(|e| e.key.clone())
        .collect();
    assert!(
        !libsecret_keys_after.contains(&"default/drop.secret".to_string())
            && !libsecret_keys_after.contains(&"default/drop.meta".to_string()),
        "drop item's keys must be absent after delete, got {libsecret_keys_after:?}",
    );
    assert!(
        libsecret_keys_after.contains(&"default/keep.secret".to_string()),
        "keep.secret must still be listed, got {libsecret_keys_after:?}",
    );

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
