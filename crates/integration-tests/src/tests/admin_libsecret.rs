//! Integration coverage for the structured libsecret admin surface
//! under `/v1/admin/stores/{sid}/resources/{rid}/libsecret/*`.

use super::*;
use base64::Engine;
use reqwest::StatusCode;
use underwrap_protocol::service::{
    AdminLibsecretAliasDeleteResponse, AdminLibsecretAliasUpsertRequest,
    AdminLibsecretCollectionCreateRequest, AdminLibsecretCollectionCreateResponse,
    AdminLibsecretCollectionDeleteResponse, AdminLibsecretItemCreateRequest,
    AdminLibsecretItemCreateResponse, AdminLibsecretItemDeleteResponse,
    AdminLibsecretItemSecretResponse, AdminLibsecretTreeResponse,
};

fn b64(value: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(value)
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
