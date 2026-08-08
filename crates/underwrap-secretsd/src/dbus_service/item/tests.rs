use super::*;
use crate::dbus_service::test_support::service_with_publisher;

#[tokio::test]
async fn set_secret_bumps_modified_and_publishes_state() {
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

    let (service, store_arc, publisher, published, responder) = service_with_publisher(store);
    let item_interface = ItemInterface::new(
        item_id.clone(),
        "default".to_string(),
        store_arc.clone(),
        publisher,
    );

    let secret = super::super::Secret {
        session: ObjectPath::try_from("/").unwrap().into(),
        parameters: vec![],
        value: b"updated".to_vec(),
        content_type: "text/plain".to_string(),
    };
    item_interface.set_secret(secret).await.unwrap();

    let store_guard = store_arc.read().await;
    let item = store_guard
        .collections
        .get("default")
        .and_then(|c| c.items.get(&item_id))
        .unwrap();
    assert!(item.modified > 1);
    assert_eq!(item.secret, b"updated");
    drop(store_guard);

    let section = published.lock().unwrap().last().unwrap().clone();
    let item = section
        .collections
        .get("default")
        .and_then(|c| c.items.get(&item_id))
        .unwrap();
    assert_eq!(item.secret.as_slice(), b"updated");

    drop(item_interface);
    drop(service);
    responder.join().unwrap();
}
