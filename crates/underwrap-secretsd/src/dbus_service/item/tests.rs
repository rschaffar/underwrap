use super::*;
use crate::test_support::{HostResponse, service_with_script};

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

#[tokio::test]
async fn delete_reject_preserves_live_state() {
    let (item_id, initial) = store_with_item();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Reject]);
    let item = interface(&item_id, store.clone(), script.publisher());

    let result = item.delete().await;
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
    let (item_id, initial) = store_with_item();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Reject]);
    let item = interface(&item_id, store.clone(), script.publisher());

    let result = item.set_secret(secret(b"synthetic-rejected-secret")).await;
    let after = store.read().await.clone();
    let published = script.finish();

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert_eq!(
        published[0].section.collections["default"].items[&item_id]
            .secret
            .as_slice(),
        b"synthetic-rejected-secret"
    );
}

#[tokio::test]
async fn attributes_reject_preserves_live_state() {
    let (item_id, initial) = store_with_item();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Reject]);
    let mut item = interface(&item_id, store.clone(), script.publisher());
    let rejected = HashMap::from([("rejected_key".into(), "rejected_value".into())]);

    let result = item.set_attributes(rejected.clone()).await;
    let after = store.read().await.clone();
    let published = script.finish();

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert_eq!(
        published[0].section.collections["default"].items[&item_id].attributes,
        rejected.into_iter().collect()
    );
}

#[tokio::test]
async fn label_reject_preserves_live_state() {
    let (item_id, initial) = store_with_item();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Reject]);
    let mut item = interface(&item_id, store.clone(), script.publisher());

    let result = item.set_label("rejected_label".into()).await;
    let after = store.read().await.clone();
    let published = script.finish();

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert_eq!(
        published[0].section.collections["default"].items[&item_id].label,
        "rejected_label"
    );
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
    let (item_id, mut initial) = store_with_item();
    initial
        .collections
        .get_mut("default")
        .unwrap()
        .items
        .get_mut(&item_id)
        .unwrap()
        .modified = 1;
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Ack]);
    let item = interface(&item_id, store.clone(), script.publisher());

    let result = item.set_secret(secret(b"synthetic-accepted-update")).await;
    let accepted = store.read().await.clone();
    let published = script.finish();
    let accepted_item = &accepted.collections["default"].items[&item_id];
    let published_item = &published[0].section.collections["default"].items[&item_id];

    assert!(result.is_ok());
    assert!(accepted_item.modified > 1);
    assert_eq!(accepted_item.secret, b"synthetic-accepted-update");
    assert_eq!(accepted_item.content_type, "application/octet-stream");
    assert_eq!(published[0].generation, 0);
    assert_eq!(
        published_item.secret.as_slice(),
        b"synthetic-accepted-update"
    );
    assert_eq!(published_item.content_type, "application/octet-stream");
}
