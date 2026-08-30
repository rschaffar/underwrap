use super::*;
use base64::Engine;
use underwrap_backend_store::{StorageBackend, load_resource_blob};
use underwrap_protocol::entities::{
    PAIRS_DOCUMENT_SECRET_ID, PairsDocument, RESOURCES_DOCUMENT_SECRET_ID, ResourcesDocument,
};
use underwrap_protocol::service::{
    AdminSecretReadResponse, AdminSecretWriteRequest, AdminStoreBackendKind,
    AdminStoreCreateRequest,
};

#[tokio::test]
async fn rotation_response_discard_retains_old_key_generation() {
    let daemon = TestDaemon::start(one_wrapper_config()).await.unwrap();
    let created = daemon
        .create_store(&AdminStoreCreateRequest {
            name: "rotation recovery".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
        })
        .await
        .unwrap();
    let store_id = created.store.id;
    let old_key_hex = created.key_hex.unwrap();
    let old_key = underwrap_backend_store::parse_hex_key(&old_key_hex).unwrap();
    let canonical = daemon.store_dir_for(&store_id);

    let resource = daemon
        .create_resource(&store_id, "response/discard/resource")
        .await
        .unwrap()
        .resource;
    daemon
        .create_pair(&store_id, "gh-authorized", &resource.id)
        .await
        .unwrap();
    let material = b"response-discard-resource";
    let secret_path = format!(
        "/v1/admin/stores/{store_id}/secrets/{}/env/ROTATION_VALUE",
        resource.id
    );
    let write = daemon
        .admin_request(reqwest::Method::PUT, &secret_path)
        .json(&AdminSecretWriteRequest {
            material_b64: base64::engine::general_purpose::STANDARD.encode(material),
        })
        .send()
        .await
        .unwrap();
    assert!(write.status().is_success());

    daemon
        .rotate_store_discard_response(&store_id)
        .await
        .unwrap();

    // Establish server-side completion through authenticated active APIs while
    // remaining blind to the replacement-key response body.
    let active_read = daemon
        .admin_request(reqwest::Method::GET, &secret_path)
        .send()
        .await
        .unwrap();
    assert!(active_read.status().is_success());
    let active_read: AdminSecretReadResponse = active_read.json().await.unwrap();
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(active_read.material_b64.unwrap())
            .unwrap(),
        material
    );
    let active_resources = daemon.list_resources(&store_id).await.unwrap();
    assert!(
        active_resources
            .resources
            .iter()
            .any(|item| item.id == resource.id)
    );

    let prefix = format!(".{store_id}.underwrap-rotation-");
    let workspace = std::fs::read_dir(canonical.parent().unwrap())
        .unwrap()
        .map(Result::unwrap)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&prefix)
        })
        .expect("rotation must retain a host recovery workspace");
    let retained = underwrap_backend_store::EncryptedBackend::open_existing_with_key(
        &workspace.join("backup"),
        old_key,
    )
    .unwrap();
    assert_eq!(
        retained
            .read(underwrap_backend_store::ENCRYPTED_CANARY_ID)
            .unwrap()
            .unwrap()
            .as_ref(),
        underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT
    );
    let blob = load_resource_blob(&retained, &resource.id).unwrap();
    assert_eq!(
        blob.env.get("ROTATION_VALUE").unwrap().as_ref(),
        b"response-discard-resource"
    );
    let resources: ResourcesDocument = serde_json::from_slice(
        retained
            .read(RESOURCES_DOCUMENT_SECRET_ID)
            .unwrap()
            .unwrap()
            .as_ref(),
    )
    .unwrap();
    assert!(resources.find_by_id(&resource.id).is_some());
    let pairs: PairsDocument = serde_json::from_slice(
        retained
            .read(PAIRS_DOCUMENT_SECRET_ID)
            .unwrap()
            .unwrap()
            .as_ref(),
    )
    .unwrap();
    assert!(pairs.find("gh-authorized", &resource.id).is_some());

    fn assert_hex_absent(path: &std::path::Path, key_hex: &str) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                assert_hex_absent(&entry.path(), key_hex);
            } else if let Ok(text) = std::fs::read_to_string(entry.path()) {
                assert!(!text.contains(key_hex));
            }
        }
    }
    assert_hex_absent(canonical.parent().unwrap(), &old_key_hex);

    daemon.stop().await.unwrap();
}
