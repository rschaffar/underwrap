use super::*;
use crate::dbus_service::test_support::service_with_state;
use underwrap_protocol::resource_blob::LibsecretSection;

fn store_with_collection() -> (String, SecretStore) {
    let mut store = SecretStore::new_default();
    let col_id = store.create_collection(Some("Test"), None);
    (col_id, store)
}

#[tokio::test]
async fn delete_removes_collection_and_persists_state() {
    let (col_id, store) = store_with_collection();
    // Drop the service immediately — we just need the side effects
    // (state path created, initial dump written).
    let (_service, store_arc, tempdir) = service_with_state(store);
    let state_path = tempdir.path().join("libsecret-state.jcs");

    let col = CollectionInterface::new(
        col_id.clone(),
        store_arc.clone(),
        Arc::new(state_path.clone()),
    );
    let res = col.delete().await.unwrap();
    assert_eq!(res.as_str(), "/");

    assert!(!store_arc.read().await.collections.contains_key(&col_id));

    let bytes = std::fs::read(&state_path).unwrap();
    let section = LibsecretSection::from_canonical_bytes(&bytes).unwrap();
    assert!(!section.collections.contains_key(&col_id));
}
