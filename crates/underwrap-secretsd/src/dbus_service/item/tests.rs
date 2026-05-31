use super::*;
use crate::dbus_service::test_support::service_with_state;
use underwrap_protocol::resource_blob::LibsecretSection;

#[tokio::test]
async fn set_secret_bumps_modified_and_persists_state() {
    let mut store = SecretStore::new_default();
    let item_id = store
        .create_item(
            "default",
            "Item",
            &HashMap::new(),
            b"original",
            "text/plain",
            false,
        )
        .unwrap()
        .0;
    // Force an old timestamp so we can detect the bump.
    store
        .collections
        .get_mut("default")
        .unwrap()
        .items
        .get_mut(&item_id)
        .unwrap()
        .modified = 1;

    let (_service, store_arc, tempdir) = service_with_state(store);
    let state_path = tempdir.path().join("libsecret-state.jcs");
    let item = ItemInterface::new(
        item_id.clone(),
        "default".to_string(),
        store_arc.clone(),
        Arc::new(state_path.clone()),
    );

    let secret = super::super::Secret {
        session: ObjectPath::try_from("/").unwrap().into(),
        parameters: vec![],
        value: b"updated".to_vec(),
        content_type: "text/plain".to_string(),
    };
    item.set_secret(secret).await.unwrap();

    let store_guard = store_arc.read().await;
    let item = store_guard
        .collections
        .get("default")
        .and_then(|c| c.items.get(&item_id))
        .unwrap();
    assert!(item.modified > 1);
    assert_eq!(item.secret, b"updated");

    let bytes = std::fs::read(&state_path).unwrap();
    let section = LibsecretSection::from_canonical_bytes(&bytes).unwrap();
    let item = section
        .collections
        .get("default")
        .and_then(|c| c.items.get(&item_id))
        .unwrap();
    assert_eq!(item.secret.as_slice(), b"updated");
}
