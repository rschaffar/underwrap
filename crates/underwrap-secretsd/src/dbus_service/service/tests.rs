use super::*;
use crate::dbus_service::test_support::service_with_state;
use underwrap_protocol::resource_blob::LibsecretSection;

#[tokio::test]
async fn read_and_set_alias_roundtrip() {
    let mut store = SecretStore::new_default();
    let work_id = store.create_collection(Some("Work"), None);
    let (service, store, _tempdir) = service_with_state(store);

    let path = service.read_alias("default").await.unwrap();
    assert_eq!(path.as_str(), format!("{BASE_PATH}/collection/default"));

    let work_path = collection_path(&work_id).unwrap();
    service.set_alias("work", work_path).await.unwrap();

    let alias_path = service.read_alias("work").await.unwrap();
    assert_eq!(
        alias_path.as_str(),
        format!("{BASE_PATH}/collection/{work_id}")
    );
    assert_eq!(store.read().await.get_alias("work"), Some(work_id.as_str()));

    // The state file mirrors the in-memory store.
    let bytes = std::fs::read(&*service.state_path).unwrap();
    let section = LibsecretSection::from_canonical_bytes(&bytes).unwrap();
    assert_eq!(section.aliases.get("work"), Some(&work_id));
}

#[tokio::test]
async fn search_items_and_get_secrets_use_in_memory_values() {
    let mut store = SecretStore::new_default();
    let attributes = HashMap::from([("service".to_string(), "github".to_string())]);
    let (item_id, _) = store
        .create_item(
            "default",
            "GitHub",
            &attributes,
            b"in-memory-secret",
            "text/plain",
            false,
        )
        .unwrap();

    let (service, _, _tempdir) = service_with_state(store);

    let (unlocked, locked) = service.search_items(attributes).await.unwrap();
    assert!(locked.is_empty());
    assert_eq!(unlocked.len(), 1);
    let item_path_str = unlocked[0].as_str().to_string();
    assert_eq!(
        item_path_str,
        format!("{BASE_PATH}/collection/default/{item_id}")
    );

    let session = ObjectPath::try_from(format!("{BASE_PATH}/session/s0")).unwrap();
    let secrets = service
        .get_secrets(
            vec![ObjectPath::try_from(item_path_str.as_str()).unwrap()],
            session,
        )
        .await
        .unwrap();
    let secret = secrets
        .get(&OwnedObjectPath::try_from(item_path_str.clone()).unwrap())
        .unwrap();
    assert_eq!(secret.value, b"in-memory-secret");
    assert_eq!(secret.content_type, "text/plain");
}
