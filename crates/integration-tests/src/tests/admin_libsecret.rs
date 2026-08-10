//! Integration coverage for the structured libsecret admin surface
//! under `/v1/admin/stores/{sid}/resources/{rid}/libsecret/*`.

use super::*;
use base64::Engine;
use reqwest::StatusCode;
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource_blob::{Collection, Item, ResourceBlob};
use underwrap_protocol::service::{
    AdminLibsecretAliasDeleteResponse, AdminLibsecretAliasUpsertRequest,
    AdminLibsecretCollectionCreateRequest, AdminLibsecretCollectionCreateResponse,
    AdminLibsecretCollectionDeleteResponse, AdminLibsecretItemCreateRequest,
    AdminLibsecretItemCreateResponse, AdminLibsecretItemDeleteResponse,
    AdminLibsecretItemSecretResponse, AdminLibsecretTreeResponse, AdminSecretRenameRequest,
    AdminSecretWriteRequest, ServiceErrorResponse,
};

fn b64(value: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(value)
}

async fn assert_resource_too_large(response: reqwest::Response) {
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let error: ServiceErrorResponse = response.json().await.unwrap();
    let maximum = underwrap_protocol::resource_blob::MAX_CANONICAL_RESOURCE_BYTES;
    assert_eq!(error.code, "resource_too_large");
    assert_eq!(
        error.message,
        format!(
            "complete canonical resource exceeds public maximum of {maximum} bytes (got {})",
            maximum + 1
        ),
        "the stable diagnostic must name only the public maximum and attempted complete length",
    );
}

fn libsecret_path(rid: &str, suffix: &str) -> String {
    format!("/v1/admin/stores/test/resources/{rid}/libsecret{suffix}")
}

async fn create_resource(daemon: &TestDaemon, name: &str) -> String {
    daemon
        .create_resource("test", name)
        .await
        .unwrap()
        .resource
        .id
}

async fn get_tree(daemon: &TestDaemon, rid: &str) -> AdminLibsecretTreeResponse {
    let resp = daemon
        .admin_request(reqwest::Method::GET, &libsecret_path(rid, ""))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    resp.json().await.unwrap()
}

#[tokio::test]
async fn empty_resource_returns_empty_tree() {
    let daemon = TestDaemon::start(one_wrapper_config()).await.unwrap();
    let rid = create_resource(&daemon, "lib/empty").await;

    let tree = get_tree(&daemon, &rid).await;
    assert!(tree.aliases.is_empty());
    assert!(tree.collections.is_empty());

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn collection_create_delete_round_trip() {
    let daemon = TestDaemon::start(one_wrapper_config()).await.unwrap();
    let rid = create_resource(&daemon, "lib/colcrud").await;

    // Create with explicit id.
    let resp = daemon
        .admin_request(reqwest::Method::POST, &libsecret_path(&rid, "/collections"))
        .json(&AdminLibsecretCollectionCreateRequest {
            id: Some("default".into()),
            label: "Default".into(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let created: AdminLibsecretCollectionCreateResponse = resp.json().await.unwrap();
    assert_eq!(created.id, "default");

    // Tree shows the collection.
    let tree = get_tree(&daemon, &rid).await;
    assert_eq!(tree.collections.len(), 1);
    assert_eq!(tree.collections[0].id, "default");
    assert_eq!(tree.collections[0].label, "Default");
    assert_eq!(
        tree.collections[0].created, tree.collections[0].modified,
        "fresh collection has created==modified"
    );

    // Delete; aliases_scrubbed empty since no alias was set.
    let resp = daemon
        .admin_request(
            reqwest::Method::DELETE,
            &libsecret_path(&rid, "/collections/default"),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: AdminLibsecretCollectionDeleteResponse = resp.json().await.unwrap();
    assert!(body.existed);
    assert!(body.aliases_scrubbed.is_empty());

    let tree = get_tree(&daemon, &rid).await;
    assert!(tree.collections.is_empty());

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn collection_delete_scrubs_pointing_aliases() {
    let daemon = TestDaemon::start(one_wrapper_config()).await.unwrap();
    let rid = create_resource(&daemon, "lib/colalias").await;

    // Seed a collection.
    daemon
        .admin_request(reqwest::Method::POST, &libsecret_path(&rid, "/collections"))
        .json(&AdminLibsecretCollectionCreateRequest {
            id: Some("default".into()),
            label: "Default".into(),
        })
        .send()
        .await
        .unwrap();

    // Set two aliases pointing at it.
    for name in ["default", "login"] {
        let resp = daemon
            .admin_request(
                reqwest::Method::PUT,
                &libsecret_path(&rid, &format!("/aliases/{name}")),
            )
            .json(&AdminLibsecretAliasUpsertRequest {
                collection_id: "default".into(),
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "alias '{name}' PUT");
    }

    // Sanity-check the tree.
    let tree = get_tree(&daemon, &rid).await;
    assert_eq!(tree.aliases.len(), 2);

    // Delete the collection; both aliases must be reported as scrubbed.
    let resp = daemon
        .admin_request(
            reqwest::Method::DELETE,
            &libsecret_path(&rid, "/collections/default"),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: AdminLibsecretCollectionDeleteResponse = resp.json().await.unwrap();
    assert!(body.existed);
    let mut scrubbed = body.aliases_scrubbed;
    scrubbed.sort();
    assert_eq!(scrubbed, vec!["default".to_string(), "login".to_string()]);

    let tree = get_tree(&daemon, &rid).await;
    assert!(tree.aliases.is_empty());
    assert!(tree.collections.is_empty());

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn item_create_reveal_delete_round_trip() {
    let daemon = TestDaemon::start(one_wrapper_config()).await.unwrap();
    let rid = create_resource(&daemon, "lib/itemcrud").await;

    // Need a collection first.
    daemon
        .admin_request(reqwest::Method::POST, &libsecret_path(&rid, "/collections"))
        .json(&AdminLibsecretCollectionCreateRequest {
            id: Some("default".into()),
            label: "Default".into(),
        })
        .send()
        .await
        .unwrap();

    // Create item with auto-id.
    let resp = daemon
        .admin_request(
            reqwest::Method::POST,
            &libsecret_path(&rid, "/collections/default/items"),
        )
        .json(&AdminLibsecretItemCreateRequest {
            id: None,
            label: "GitHub Token".into(),
            content_type: "text/plain".into(),
            attributes: [("service".to_string(), "github.com".to_string())].into(),
            secret_b64: b64("ghp_abc"),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let created: AdminLibsecretItemCreateResponse = resp.json().await.unwrap();
    assert_eq!(created.id, "item_1");

    // Reveal returns the stored secret.
    let resp = daemon
        .admin_request(
            reqwest::Method::GET,
            &libsecret_path(&rid, "/collections/default/items/item_1/secret"),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: AdminLibsecretItemSecretResponse = resp.json().await.unwrap();
    assert_eq!(body.secret_b64, b64("ghp_abc"));

    // Delete item.
    let resp = daemon
        .admin_request(
            reqwest::Method::DELETE,
            &libsecret_path(&rid, "/collections/default/items/item_1"),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: AdminLibsecretItemDeleteResponse = resp.json().await.unwrap();
    assert!(body.existed);

    let tree = get_tree(&daemon, &rid).await;
    assert!(tree.collections[0].items.is_empty());

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn alias_put_fails_when_collection_unknown() {
    let daemon = TestDaemon::start(one_wrapper_config()).await.unwrap();
    let rid = create_resource(&daemon, "lib/aliasmiss").await;

    let resp = daemon
        .admin_request(
            reqwest::Method::PUT,
            &libsecret_path(&rid, "/aliases/default"),
        )
        .json(&AdminLibsecretAliasUpsertRequest {
            collection_id: "no-such-collection".into(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let tree = get_tree(&daemon, &rid).await;
    assert!(tree.aliases.is_empty());

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn alias_put_then_delete_round_trip() {
    let daemon = TestDaemon::start(one_wrapper_config()).await.unwrap();
    let rid = create_resource(&daemon, "lib/aliascrud").await;

    daemon
        .admin_request(reqwest::Method::POST, &libsecret_path(&rid, "/collections"))
        .json(&AdminLibsecretCollectionCreateRequest {
            id: Some("default".into()),
            label: "Default".into(),
        })
        .send()
        .await
        .unwrap();

    daemon
        .admin_request(
            reqwest::Method::PUT,
            &libsecret_path(&rid, "/aliases/default"),
        )
        .json(&AdminLibsecretAliasUpsertRequest {
            collection_id: "default".into(),
        })
        .send()
        .await
        .unwrap();

    let tree = get_tree(&daemon, &rid).await;
    assert_eq!(
        tree.aliases.get("default").map(String::as_str),
        Some("default")
    );

    let resp = daemon
        .admin_request(
            reqwest::Method::DELETE,
            &libsecret_path(&rid, "/aliases/default"),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: AdminLibsecretAliasDeleteResponse = resp.json().await.unwrap();
    assert!(body.existed);

    let tree = get_tree(&daemon, &rid).await;
    assert!(tree.aliases.is_empty());

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn collection_create_rejects_leading_underscore_id() {
    let daemon = TestDaemon::start(one_wrapper_config()).await.unwrap();
    let rid = create_resource(&daemon, "lib/validation").await;

    let resp = daemon
        .admin_request(reqwest::Method::POST, &libsecret_path(&rid, "/collections"))
        .json(&AdminLibsecretCollectionCreateRequest {
            id: Some("_meta".into()),
            label: "should fail".into(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn alias_put_rejects_leading_underscore_name() {
    // The alias name lives in the URL path; reaching the handler still
    // counts as an admin attempt, so the validator must reject it
    // before mutating the blob.
    let daemon = TestDaemon::start(one_wrapper_config()).await.unwrap();
    let rid = create_resource(&daemon, "lib/aliasvalid").await;

    // Seed a real collection so the failure is the alias-name shape,
    // not the unknown-collection 404.
    daemon
        .admin_request(reqwest::Method::POST, &libsecret_path(&rid, "/collections"))
        .json(&AdminLibsecretCollectionCreateRequest {
            id: Some("default".into()),
            label: "Default".into(),
        })
        .send()
        .await
        .unwrap();

    let resp = daemon
        .admin_request(
            reqwest::Method::PUT,
            &libsecret_path(&rid, "/aliases/_reserved"),
        )
        .json(&AdminLibsecretAliasUpsertRequest {
            collection_id: "default".into(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let tree = get_tree(&daemon, &rid).await;
    assert!(tree.aliases.is_empty());

    daemon.stop().await.unwrap();
}

fn save_boundary_seed(daemon: &TestDaemon, resource_id: &str, seed: &ResourceBlob) {
    daemon.put_resource_blob("test", resource_id, seed).unwrap();
}

fn assert_exact_max_saved(daemon: &TestDaemon, resource_id: &str) {
    assert_eq!(
        daemon
            .get_resource_blob("test", resource_id)
            .unwrap()
            .to_canonical_bytes()
            .unwrap()
            .len(),
        underwrap_protocol::resource_blob::MAX_CANONICAL_RESOURCE_BYTES
    );
}

#[tokio::test]
async fn material_adding_admin_routes_enforce_exact_complete_resource_limit() {
    let daemon = TestDaemon::start(one_wrapper_config()).await.unwrap();
    let rid = create_resource(&daemon, "size/routes").await;
    let max = underwrap_protocol::resource_blob::MAX_CANONICAL_RESOURCE_BYTES;

    for slot in ["env", "files"] {
        let mut candidate = ResourceBlob::empty();
        let entries = if slot == "env" {
            &mut candidate.env
        } else {
            &mut candidate.files
        };
        entries.insert("a".into(), SecretBytes::from(Vec::<u8>::new()));
        let candidate = pad_blob_to_exact_len(candidate, max);
        let mut seed = candidate.clone();
        if slot == "env" {
            seed.env.remove("a");
        } else {
            seed.files.remove("a");
        }
        save_boundary_seed(&daemon, &rid, &seed);
        let response = daemon
            .admin_request(
                reqwest::Method::PUT,
                &format!("/v1/admin/stores/test/secrets/{rid}/{slot}/a"),
            )
            .json(&AdminSecretWriteRequest {
                material_b64: String::new(),
            })
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_exact_max_saved(&daemon, &rid);

        save_boundary_seed(&daemon, &rid, &seed);
        let response = daemon
            .admin_request(
                reqwest::Method::PUT,
                &format!("/v1/admin/stores/test/secrets/{rid}/{slot}/aa"),
            )
            .json(&AdminSecretWriteRequest {
                material_b64: String::new(),
            })
            .send()
            .await
            .unwrap();
        assert_resource_too_large(response).await;
        assert_eq!(daemon.get_resource_blob("test", &rid).unwrap(), seed);
    }

    for slot in ["env", "files"] {
        let mut candidate = ResourceBlob::empty();
        let entries = if slot == "env" {
            &mut candidate.env
        } else {
            &mut candidate.files
        };
        entries.insert("a".into(), SecretBytes::from(Vec::<u8>::new()));
        let candidate = pad_blob_to_exact_len(candidate, max);
        let mut seed = candidate.clone();
        if slot == "env" {
            seed.env.remove("a");
            seed.env
                .insert("z".into(), SecretBytes::from(Vec::<u8>::new()));
        } else {
            seed.files.remove("a");
            seed.files
                .insert("z".into(), SecretBytes::from(Vec::<u8>::new()));
        }
        assert_eq!(seed.to_canonical_bytes().unwrap().len(), max);
        save_boundary_seed(&daemon, &rid, &seed);
        let response = daemon
            .admin_request(
                reqwest::Method::POST,
                &format!("/v1/admin/stores/test/secrets/{rid}/{slot}/z/rename"),
            )
            .json(&AdminSecretRenameRequest {
                to_key: "a".into(),
                material_b64: None,
            })
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_exact_max_saved(&daemon, &rid);

        save_boundary_seed(&daemon, &rid, &seed);
        let response = daemon
            .admin_request(
                reqwest::Method::POST,
                &format!("/v1/admin/stores/test/secrets/{rid}/{slot}/z/rename"),
            )
            .json(&AdminSecretRenameRequest {
                to_key: "aa".into(),
                material_b64: None,
            })
            .send()
            .await
            .unwrap();
        assert_resource_too_large(response).await;
        assert_eq!(daemon.get_resource_blob("test", &rid).unwrap(), seed);
    }

    let mut collection_candidate = ResourceBlob::empty();
    collection_candidate.libsecret.collections.insert(
        "a".into(),
        Collection {
            label: "new".into(),
            created: 1_700_000_000,
            modified: 1_700_000_000,
            items: std::collections::BTreeMap::new(),
        },
    );
    let mut collection_seed = pad_blob_to_exact_len(collection_candidate, max);
    collection_seed.libsecret.collections.remove("a");
    save_boundary_seed(&daemon, &rid, &collection_seed);
    let response = daemon
        .admin_request(reqwest::Method::POST, &libsecret_path(&rid, "/collections"))
        .json(&AdminLibsecretCollectionCreateRequest {
            id: Some("a".into()),
            label: "new".into(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_exact_max_saved(&daemon, &rid);
    save_boundary_seed(&daemon, &rid, &collection_seed);
    let response = daemon
        .admin_request(reqwest::Method::POST, &libsecret_path(&rid, "/collections"))
        .json(&AdminLibsecretCollectionCreateRequest {
            id: Some("aa".into()),
            label: "new".into(),
        })
        .send()
        .await
        .unwrap();
    assert_resource_too_large(response).await;
    assert_eq!(
        daemon.get_resource_blob("test", &rid).unwrap(),
        collection_seed
    );

    let exact_item_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let oversized_item_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mut item_candidate = ResourceBlob::empty();
    item_candidate.libsecret.collections.insert(
        "c".into(),
        Collection {
            label: "collection".into(),
            created: 1_700_000_000,
            modified: 1_700_000_000,
            items: std::collections::BTreeMap::from([(
                exact_item_id.into(),
                Item {
                    label: "item".into(),
                    content_type: "text/plain".into(),
                    created: 1_700_000_000,
                    modified: 1_700_000_000,
                    attributes: std::collections::BTreeMap::new(),
                    secret: SecretBytes::from(Vec::<u8>::new()),
                },
            )]),
        },
    );
    let mut item_seed = pad_blob_to_exact_len(item_candidate, max);
    item_seed
        .libsecret
        .collections
        .get_mut("c")
        .unwrap()
        .items
        .remove(exact_item_id);
    let item_body = |id: &str| AdminLibsecretItemCreateRequest {
        id: Some(id.into()),
        label: "item".into(),
        content_type: "text/plain".into(),
        attributes: std::collections::BTreeMap::new(),
        secret_b64: String::new(),
    };
    save_boundary_seed(&daemon, &rid, &item_seed);
    let response = daemon
        .admin_request(
            reqwest::Method::POST,
            &libsecret_path(&rid, "/collections/c/items"),
        )
        .json(&item_body(exact_item_id))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_exact_max_saved(&daemon, &rid);
    save_boundary_seed(&daemon, &rid, &item_seed);
    let response = daemon
        .admin_request(
            reqwest::Method::POST,
            &libsecret_path(&rid, "/collections/c/items"),
        )
        .json(&item_body(oversized_item_id))
        .send()
        .await
        .unwrap();
    assert_resource_too_large(response).await;
    assert_eq!(daemon.get_resource_blob("test", &rid).unwrap(), item_seed);

    let mut alias_candidate = ResourceBlob::empty();
    alias_candidate.libsecret.collections.insert(
        "c".into(),
        Collection {
            label: "collection".into(),
            created: 1_700_000_000,
            modified: 1_700_000_000,
            items: std::collections::BTreeMap::new(),
        },
    );
    alias_candidate
        .libsecret
        .aliases
        .insert("a".into(), "c".into());
    let mut alias_seed = pad_blob_to_exact_len(alias_candidate, max);
    alias_seed.libsecret.aliases.remove("a");
    for (name, success) in [("a", true), ("aa", false)] {
        save_boundary_seed(&daemon, &rid, &alias_seed);
        let response = daemon
            .admin_request(
                reqwest::Method::PUT,
                &libsecret_path(&rid, &format!("/aliases/{name}")),
            )
            .json(&AdminLibsecretAliasUpsertRequest {
                collection_id: "c".into(),
            })
            .send()
            .await
            .unwrap();
        if success {
            assert_eq!(response.status(), StatusCode::OK);
            assert_exact_max_saved(&daemon, &rid);
        } else {
            assert_resource_too_large(response).await;
            assert_eq!(daemon.get_resource_blob("test", &rid).unwrap(), alias_seed);
        }
    }

    daemon.stop().await.unwrap();
}
