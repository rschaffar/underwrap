use super::*;
use base64::Engine;
use std::collections::BTreeMap;
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource_blob::ResourceBlob;

#[tokio::test]
async fn resource_blob_get_returns_canonical_bytes_for_empty_resource() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    let resp = daemon
        .get_resource_blob("test", "github/token")
        .await
        .unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&resp.blob_b64)
        .unwrap();
    let blob = ResourceBlob::from_canonical_bytes(&bytes).unwrap();
    assert_eq!(blob, ResourceBlob::empty());

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn resource_blob_get_then_put_round_trips() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    let mut blob = ResourceBlob::empty();
    let mut env = BTreeMap::new();
    env.insert("GH_TOKEN".into(), SecretBytes::from("ghp_blob_round_trip"));
    blob.env = env;

    let bytes = blob.to_canonical_bytes().unwrap();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let put = daemon
        .put_resource_blob("test", "github/token", &b64)
        .await
        .unwrap();
    assert!(put.status().is_success(), "PUT failed: {}", put.status());

    let got = daemon
        .get_resource_blob("test", "github/token")
        .await
        .unwrap();
    let got_bytes = base64::engine::general_purpose::STANDARD
        .decode(&got.blob_b64)
        .unwrap();
    let got_blob = ResourceBlob::from_canonical_bytes(&got_bytes).unwrap();
    assert_eq!(got_blob, blob);

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn resource_blob_put_rejects_noncanonical_bytes() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    // Hand-crafted JSON with keys in non-canonical order. Valid as
    // JSON, valid as a ResourceBlob, but the bytes != JCS form.
    let noncanonical =
        br#"{"schema_version":1,"libsecret":{"aliases":{},"collections":{}},"files":{},"env":{}}"#;
    let b64 = base64::engine::general_purpose::STANDARD.encode(noncanonical);
    let put = daemon
        .put_resource_blob("test", "github/token", &b64)
        .await
        .unwrap();
    assert_eq!(put.status(), reqwest::StatusCode::BAD_REQUEST);
    let body = put.text().await.unwrap();
    assert!(
        body.contains("noncanonical_blob"),
        "expected noncanonical_blob error, got: {body}",
    );

    daemon.stop().await.unwrap();
}
