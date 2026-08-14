use super::*;
use crate::BASE_PATH;
use crate::publication::Publisher;
use crate::test_support::{HostResponse, ObjectServerHarness, service_with_script};
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::time::Duration;
use underwrap_protocol::libsecret_publication::{
    REQUEST_HEADER_LEN, ResponseKind, decode_request_header, encode_response,
};

fn store_with_collection() -> SecretStore {
    let mut store = SecretStore::new_default();
    store.create_collection("Test", None);
    store
}

fn item_properties(label: &str) -> HashMap<String, Value<'static>> {
    HashMap::from([(
        "org.freedesktop.Secret.Item.Label".to_string(),
        Value::from(label.to_string()),
    )])
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
async fn delete_ack_removes_collection_and_publishes_state() {
    let initial = store_with_collection();
    let (service, store, script) = service_with_script(initial, vec![HostResponse::Ack]);
    let collection = CollectionInterface::new("Test".into(), store.clone(), script.publisher());

    let result = collection.delete().await;
    let accepted = store.read().await.clone();
    let published = script.finish();
    drop(service);

    assert_eq!(result.unwrap().as_str(), "/");
    assert!(!accepted.collections.contains_key("Test"));
    assert!(!published[0].section.collections.contains_key("Test"));
}

#[tokio::test]
async fn delete_reject_preserves_live_state() {
    let mut initial = store_with_collection();
    initial
        .create_item(
            "Test",
            "synthetic",
            &HashMap::new(),
            b"synthetic-delete-value",
            "text/plain",
            false,
        )
        .unwrap();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Reject]);
    let collection = CollectionInterface::new("Test".into(), store.clone(), script.publisher());

    let result = collection.delete().await;
    let after = store.read().await.clone();
    let published = script.finish();

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert!(!published[0].section.collections.contains_key("Test"));
}

#[tokio::test]
async fn create_item_new_reject_preserves_live_state_and_registers_nothing() {
    let initial = store_with_collection();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Reject]);
    let collection = CollectionInterface::new("Test".into(), store.clone(), script.publisher());
    let object_server = ObjectServerHarness::new().await;

    let result = {
        let server = object_server.object_server();
        collection
            .create_item(
                item_properties("rejected_new"),
                secret(b"synthetic-rejected-new"),
                false,
                &server,
            )
            .await
    };
    let after = store.read().await.clone();
    let published = script.finish();
    let rejected_id = published[0].section.collections["Test"]
        .items
        .keys()
        .next()
        .unwrap()
        .clone();
    let rejected_path = format!("{BASE_PATH}/collection/Test/{rejected_id}");
    let registered = object_server.has_item(&rejected_path).await;
    object_server.close().await;

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert!(!registered);
}

#[tokio::test]
async fn create_item_replace_reject_preserves_live_state() {
    let mut initial = store_with_collection();
    let (item_id, _) = initial
        .create_item(
            "Test",
            "accepted",
            &HashMap::new(),
            b"synthetic-accepted-before-replace",
            "text/plain",
            false,
        )
        .unwrap();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Reject]);
    let collection = CollectionInterface::new("Test".into(), store.clone(), script.publisher());
    let object_server = ObjectServerHarness::new().await;
    let item_path = format!("{BASE_PATH}/collection/Test/{item_id}");
    {
        let server = object_server.object_server();
        server
            .at(
                item_path.as_str(),
                ItemInterface::new(
                    item_id.clone(),
                    "Test".into(),
                    store.clone(),
                    script.publisher(),
                ),
            )
            .await
            .unwrap();
    }

    let result = {
        let server = object_server.object_server();
        collection
            .create_item(
                item_properties("rejected_replacement"),
                secret(b"synthetic-rejected-replacement"),
                true,
                &server,
            )
            .await
    };
    let registered = object_server.has_item(&item_path).await;
    let after = store.read().await.clone();
    object_server.close().await;
    let published = script.finish();

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert!(registered);
    assert_eq!(published[0].section.collections["Test"].items.len(), 1);
    assert_eq!(
        published[0].section.collections["Test"].items[&item_id].label,
        "rejected_replacement"
    );
}

#[tokio::test]
async fn collection_label_reject_preserves_live_state() {
    let initial = store_with_collection();
    let before = initial.clone();
    let (_service, store, script) = service_with_script(initial, vec![HostResponse::Reject]);
    let mut collection = CollectionInterface::new("Test".into(), store.clone(), script.publisher());

    let result = collection.set_label("rejected_label".into()).await;
    let after = store.read().await.clone();
    let published = script.finish();

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert_eq!(
        published[0].section.collections["Test"].label,
        "rejected_label"
    );
}

#[tokio::test]
async fn create_item_ack_registers_new_path_and_replace_reuses_it() {
    let initial = store_with_collection();
    let (_service, store, script) =
        service_with_script(initial, vec![HostResponse::Ack, HostResponse::Ack]);
    let collection = CollectionInterface::new("Test".into(), store.clone(), script.publisher());
    let object_server = ObjectServerHarness::new().await;

    let created = {
        let server = object_server.object_server();
        collection
            .create_item(
                item_properties("accepted_new"),
                secret(b"synthetic-accepted-new"),
                false,
                &server,
            )
            .await
    };
    let created_path = created.as_ref().unwrap().0.as_str().to_string();
    let registered_after_create = object_server.has_item(&created_path).await;
    let replaced = {
        let server = object_server.object_server();
        collection
            .create_item(
                item_properties("accepted_replacement"),
                secret(b"synthetic-accepted-replacement"),
                true,
                &server,
            )
            .await
    };
    let registered_after_replace = object_server.has_item(&created_path).await;
    let accepted = store.read().await.clone();
    object_server.close().await;
    let published = script.finish();

    let (created_path_result, created_prompt) = created.unwrap();
    let (replaced_path, replaced_prompt) = replaced.unwrap();
    assert_eq!(created_path_result.as_str(), created_path);
    assert_eq!(replaced_path.as_str(), created_path);
    assert_eq!(created_prompt.as_str(), "/");
    assert_eq!(replaced_prompt.as_str(), "/");
    assert!(registered_after_create);
    assert!(registered_after_replace);
    assert_eq!(accepted.collections["Test"].items.len(), 1);
    let accepted_item = accepted.collections["Test"].items.values().next().unwrap();
    assert_eq!(accepted_item.label, "accepted_replacement");
    assert_eq!(accepted_item.secret, b"synthetic-accepted-replacement");
    assert_eq!(published[0].generation, 0);
    assert_eq!(published[1].generation, 1);
    assert_eq!(published[0].section.collections["Test"].items.len(), 1);
    assert_eq!(published[1].section.collections["Test"].items.len(), 1);
}

struct ShutdownGuard(UnixStream);

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

#[test]
fn production_handler_holds_write_lock_and_returns_only_after_exact_ack() {
    let initial = store_with_collection();
    let store = Arc::new(RwLock::new(initial));
    let (publisher_stream, mut host) = UnixStream::pair().unwrap();
    host.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    host.set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let shutdown_stream = host.try_clone().unwrap();
    let publisher = Arc::new(Mutex::new(Publisher::from_stream(publisher_stream)));
    let mut collection = CollectionInterface::new("Test".into(), store.clone(), publisher);
    let (result_tx, result_rx) = std::sync::mpsc::channel();

    std::thread::scope(|scope| {
        let shutdown = ShutdownGuard(shutdown_stream);
        let mutation = scope.spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let result = runtime.block_on(collection.set_label("accepted_after_ack".into()));
            result_tx.send(result).unwrap();
        });

        let mut header = [0; REQUEST_HEADER_LEN];
        host.read_exact(&mut header).unwrap();
        let header = decode_request_header(&header).unwrap();
        let mut body = vec![0; header.body_len];
        host.read_exact(&mut body).unwrap();

        assert_eq!(header.generation, 0);
        assert!(result_rx.try_recv().is_err());
        assert!(store.try_read().is_err(), "write lock released before ACK");
        host.write_all(&encode_response(ResponseKind::Ack, 0))
            .unwrap();
        let result = result_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        host.shutdown(Shutdown::Both).unwrap();
        mutation.join().unwrap();
        drop(shutdown);
        assert!(result.is_ok());
    });

    assert_eq!(
        store.blocking_read().get_collection_label("Test"),
        Some("accepted_after_ack")
    );
}
