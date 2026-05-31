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

// Mirrors the order the `create_collection` D-Bus handler uses: validate
// the client-supplied name first, only then touch the store. Lets us test
// the name → path behaviour without standing up a real D-Bus object server.
fn try_create(store: &mut SecretStore, label: &str) -> Result<String, &'static str> {
    validate_collection_name(label)?;
    Ok(store.create_collection(Some(label), None))
}

#[test]
fn create_collection_path_is_derived_from_name() {
    for name in ["gogcli", "awsssocli"] {
        let mut store = SecretStore::new_default();
        let id = try_create(&mut store, name).unwrap();
        let path = collection_path(&id).unwrap();
        assert_eq!(
            path.as_str(),
            format!("{BASE_PATH}/collection/{name}"),
            "Go keyring clients locate the collection by this exact path"
        );
    }
}

#[test]
fn create_collection_same_name_is_idempotent() {
    let mut store = SecretStore::new_default();
    let first = try_create(&mut store, "gogcli").unwrap();
    let before = store.collections.len();
    let second = try_create(&mut store, "gogcli").unwrap();
    assert_eq!(first, second, "same name returns the same collection");
    assert_eq!(
        store.collections.len(),
        before,
        "no fresh col_N litter on repeat creates"
    );
}

#[test]
fn create_collection_accepts_real_client_names() {
    for name in ["gogcli", "awsssocli", "login", "default", "col_1", "9to5"] {
        assert!(
            validate_collection_name(name).is_ok(),
            "{name:?} should be a valid collection name"
        );
    }
}

#[test]
fn create_collection_rejects_invalid_names_and_creates_nothing() {
    let too_long = "x".repeat(MAX_COLLECTION_NAME_LEN + 1);
    for bad in ["", "a/b", "naïve", "a b", "a.b", "a-b", too_long.as_str()] {
        assert!(
            validate_collection_name(bad).is_err(),
            "{bad:?} must be rejected"
        );
        let mut store = SecretStore::new_default();
        let before = store.collections.len();
        assert!(try_create(&mut store, bad).is_err());
        assert_eq!(
            store.collections.len(),
            before,
            "{bad:?} must not persist a collection"
        );
    }
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
