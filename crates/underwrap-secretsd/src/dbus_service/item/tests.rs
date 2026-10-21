use super::*;
use crate::test_support::{HostResponse, service_with_script};

const ITEM_MODIFIED_BASELINE: u64 = 11;
const COLLECTION_MODIFIED_BASELINE: u64 = 17;

fn store_with_item() -> (String, SecretStore) {
    let mut store = SecretStore::new_default();
    let item_id = store
        .create_item(
            "default",
            "accepted_label",
            &HashMap::from([("accepted_key".into(), "accepted_value".into())]),
            b"synthetic-accepted-item",
            "text/plain",
            false,
        )
        .unwrap()
        .0;
    (item_id, store)
}

fn store_with_backdated_item() -> (String, SecretStore) {
    let (item_id, mut store) = store_with_item();
    let collection = store.collections.get_mut("default").unwrap();
    collection.modified = COLLECTION_MODIFIED_BASELINE;
    collection.items.get_mut(&item_id).unwrap().modified = ITEM_MODIFIED_BASELINE;
    (item_id, store)
}

fn interface(
    item_id: &str,
    store: Arc<RwLock<SecretStore>>,
    publisher: crate::publication::SharedPublisher,
) -> ItemInterface {
    ItemInterface::new(item_id.to_string(), "default".into(), store, publisher)
}

fn secret(value: &[u8]) -> Secret {
    Secret {
        session: ObjectPath::try_from("/").unwrap().into(),
        parameters: vec![],
        value: value.to_vec(),
        content_type: "application/octet-stream".to_string(),
    }
}

fn ordered(map: &HashMap<String, String>) -> std::collections::BTreeMap<String, String> {
    map.iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[tokio::test]
async fn delete_reject_preserves_live_state() {
    let (item_id, initial) = store_with_item();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Reject]);
    let item = interface(&item_id, store.clone(), script.publisher());

    let object_server = crate::test_support::ObjectServerHarness::new().await;
    crate::dbus_service::register_objects(
        &object_server.object_server(),
        &store,
        &script.publisher(),
    )
    .await
    .unwrap();
    let result = item.delete(&object_server.object_server()).await;
    let after = store.read().await.clone();
    let published = script.finish();

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert!(
        !published[0].section.collections["default"]
            .items
            .contains_key(&item_id)
    );
}

#[tokio::test]
async fn set_secret_reject_preserves_live_state() {
    let (item_id, initial) = store_with_backdated_item();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Reject]);
    let item = interface(&item_id, store.clone(), script.publisher());

    let result = item.set_secret(secret(b"synthetic-rejected-secret")).await;
    let after = store.read().await.clone();
    let published = script.finish();
    let candidate_collection = &published[0].section.collections["default"];
    let candidate_item = &candidate_collection.items[&item_id];

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert_eq!(candidate_item.secret, b"synthetic-rejected-secret");
    assert_eq!(candidate_item.modified, candidate_collection.modified);
    assert!(candidate_item.modified > COLLECTION_MODIFIED_BASELINE);
}

#[tokio::test]
async fn attributes_reject_preserves_live_state() {
    let (item_id, initial) = store_with_backdated_item();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Reject]);
    let item = interface(&item_id, store.clone(), script.publisher());
    let rejected = HashMap::from([("rejected_key".into(), "rejected_value".into())]);

    let result = item.set_attributes(rejected.clone()).await;
    let after = store.read().await.clone();
    let published = script.finish();
    let candidate_collection = &published[0].section.collections["default"];
    let candidate_item = &candidate_collection.items[&item_id];

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert_eq!(candidate_item.attributes, ordered(&rejected));
    assert_eq!(candidate_item.modified, candidate_collection.modified);
    assert!(candidate_item.modified > COLLECTION_MODIFIED_BASELINE);
}

#[tokio::test]
async fn label_reject_preserves_live_state() {
    let (item_id, initial) = store_with_backdated_item();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Reject]);
    let item = interface(&item_id, store.clone(), script.publisher());

    let result = item.set_label("rejected_label".into()).await;
    let after = store.read().await.clone();
    let published = script.finish();
    let candidate_collection = &published[0].section.collections["default"];
    let candidate_item = &candidate_collection.items[&item_id];

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert_eq!(candidate_item.label, "rejected_label");
    assert_eq!(candidate_item.modified, candidate_collection.modified);
    assert!(candidate_item.modified > COLLECTION_MODIFIED_BASELINE);
}

#[tokio::test]
async fn response_eof_preserves_live_state() {
    let (item_id, initial) = store_with_item();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Eof]);
    let item = interface(&item_id, store.clone(), script.publisher());

    let result = item
        .set_secret(secret(b"synthetic-transport-failure"))
        .await;
    let after = store.read().await.clone();
    let published = script.finish();

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert_eq!(published[0].generation, 0);
}

#[tokio::test]
async fn set_secret_ack_commits_only_published_state() {
    let (item_id, initial) = store_with_backdated_item();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Ack]);
    let item = interface(&item_id, store.clone(), script.publisher());

    let result = item.set_secret(secret(b"synthetic-accepted-update")).await;
    let getter_modified = item.modified().await.unwrap();
    let accepted = store.read().await.clone();
    let published = script.finish();
    let accepted_collection = &accepted.collections["default"];
    let accepted_item = &accepted_collection.items[&item_id];
    let published_collection = &published[0].section.collections["default"];
    let published_item = &published_collection.items[&item_id];
    let before_collection = &before.collections["default"];
    let before_item = &before_collection.items[&item_id];

    assert!(result.is_ok());
    assert_eq!(accepted_item.secret, b"synthetic-accepted-update");
    assert_eq!(accepted_item.content_type, "application/octet-stream");
    assert_eq!(accepted_item.attributes, before_item.attributes);
    assert_eq!(accepted_item.label, before_item.label);
    assert_eq!(accepted_item.created, before_item.created);
    assert_eq!(accepted_collection.created, before_collection.created);
    assert_eq!(accepted_collection.label, before_collection.label);
    assert_eq!(accepted_item.modified, accepted_collection.modified);
    assert!(accepted_item.modified > COLLECTION_MODIFIED_BASELINE);
    assert_eq!(getter_modified, accepted_item.modified);
    assert_eq!(
        published_item.secret.as_slice(),
        accepted_item.secret.as_slice()
    );
    assert_eq!(published_item.content_type, accepted_item.content_type);
    assert_eq!(
        published_item.attributes,
        ordered(&accepted_item.attributes)
    );
    assert_eq!(published_item.label, accepted_item.label);
    assert_eq!(published_item.created, accepted_item.created);
    assert_eq!(published_item.modified, accepted_item.modified);
    assert_eq!(published_collection.label, accepted_collection.label);
    assert_eq!(published_collection.created, accepted_collection.created);
    assert_eq!(published_collection.modified, accepted_collection.modified);
    assert_eq!(published[0].section.aliases, ordered(&accepted.aliases));
    assert_eq!(published[0].generation, 0);
}

#[tokio::test]
async fn attributes_ack_commits_only_published_state() {
    let (item_id, initial) = store_with_backdated_item();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Ack]);
    let item = interface(&item_id, store.clone(), script.publisher());
    let attributes = HashMap::from([("updated_key".into(), "updated_value".into())]);

    let result = item.set_attributes(attributes.clone()).await;
    let getter_modified = item.modified().await.unwrap();
    let accepted = store.read().await.clone();
    let published = script.finish();
    let accepted_collection = &accepted.collections["default"];
    let accepted_item = &accepted_collection.items[&item_id];
    let published_collection = &published[0].section.collections["default"];
    let published_item = &published_collection.items[&item_id];
    let before_collection = &before.collections["default"];
    let before_item = &before_collection.items[&item_id];

    assert!(result.is_ok());
    assert_eq!(accepted_item.attributes, attributes);
    assert_eq!(accepted_item.secret, before_item.secret);
    assert_eq!(accepted_item.content_type, before_item.content_type);
    assert_eq!(accepted_item.label, before_item.label);
    assert_eq!(accepted_item.created, before_item.created);
    assert_eq!(accepted_collection.created, before_collection.created);
    assert_eq!(accepted_collection.label, before_collection.label);
    assert_eq!(accepted_item.modified, accepted_collection.modified);
    assert!(accepted_item.modified > COLLECTION_MODIFIED_BASELINE);
    assert_eq!(getter_modified, accepted_item.modified);
    assert_eq!(
        published_item.secret.as_slice(),
        accepted_item.secret.as_slice()
    );
    assert_eq!(published_item.content_type, accepted_item.content_type);
    assert_eq!(
        published_item.attributes,
        ordered(&accepted_item.attributes)
    );
    assert_eq!(published_item.label, accepted_item.label);
    assert_eq!(published_item.created, accepted_item.created);
    assert_eq!(published_item.modified, accepted_item.modified);
    assert_eq!(published_collection.label, accepted_collection.label);
    assert_eq!(published_collection.created, accepted_collection.created);
    assert_eq!(published_collection.modified, accepted_collection.modified);
    assert_eq!(published[0].section.aliases, ordered(&accepted.aliases));
    assert_eq!(published[0].generation, 0);
}

#[tokio::test]
async fn label_ack_commits_only_published_state() {
    let (item_id, initial) = store_with_backdated_item();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Ack]);
    let item = interface(&item_id, store.clone(), script.publisher());

    let result = item.set_label("accepted_updated_label".into()).await;
    let getter_modified = item.modified().await.unwrap();
    let accepted = store.read().await.clone();
    let published = script.finish();
    let accepted_collection = &accepted.collections["default"];
    let accepted_item = &accepted_collection.items[&item_id];
    let published_collection = &published[0].section.collections["default"];
    let published_item = &published_collection.items[&item_id];
    let before_collection = &before.collections["default"];
    let before_item = &before_collection.items[&item_id];

    assert!(result.is_ok());
    assert_eq!(accepted_item.label, "accepted_updated_label");
    assert_eq!(accepted_item.secret, before_item.secret);
    assert_eq!(accepted_item.content_type, before_item.content_type);
    assert_eq!(accepted_item.attributes, before_item.attributes);
    assert_eq!(accepted_item.created, before_item.created);
    assert_eq!(accepted_collection.created, before_collection.created);
    assert_eq!(accepted_collection.label, before_collection.label);
    assert_eq!(accepted_item.modified, accepted_collection.modified);
    assert!(accepted_item.modified > COLLECTION_MODIFIED_BASELINE);
    assert_eq!(getter_modified, accepted_item.modified);
    assert_eq!(
        published_item.secret.as_slice(),
        accepted_item.secret.as_slice()
    );
    assert_eq!(published_item.content_type, accepted_item.content_type);
    assert_eq!(
        published_item.attributes,
        ordered(&accepted_item.attributes)
    );
    assert_eq!(published_item.label, accepted_item.label);
    assert_eq!(published_item.created, accepted_item.created);
    assert_eq!(published_item.modified, accepted_item.modified);
    assert_eq!(published_collection.label, accepted_collection.label);
    assert_eq!(published_collection.created, accepted_collection.created);
    assert_eq!(published_collection.modified, accepted_collection.modified);
    assert_eq!(published[0].section.aliases, ordered(&accepted.aliases));
    assert_eq!(published[0].generation, 0);
}
