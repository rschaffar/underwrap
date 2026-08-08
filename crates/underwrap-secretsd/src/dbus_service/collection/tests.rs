use super::*;
use crate::dbus_service::test_support::service_with_publisher;
use crate::publication::Publisher;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use underwrap_protocol::libsecret_publication::{
    REQUEST_HEADER_LEN, ResponseKind, decode_request_header, encode_response,
};
use underwrap_protocol::resource_blob::LibsecretSection;

fn controlled_publisher() -> (crate::publication::SharedPublisher, UnixStream) {
    let (publisher, host) = UnixStream::pair().unwrap();
    (
        Arc::new(Mutex::new(Publisher::from_stream(publisher))),
        host,
    )
}

fn read_publication(host: &mut UnixStream) -> u64 {
    let mut header = [0; REQUEST_HEADER_LEN];
    host.read_exact(&mut header).unwrap();
    let header = decode_request_header(&header).unwrap();
    let mut body = vec![0; header.body_len];
    host.read_exact(&mut body).unwrap();
    LibsecretSection::from_strict_publication_bytes(&body).unwrap();
    header.generation
}

fn store_with_collection() -> (String, SecretStore) {
    let mut store = SecretStore::new_default();
    let col_id = store.create_collection("Test", None);
    (col_id, store)
}

#[tokio::test]
async fn delete_removes_collection_and_publishes_state() {
    let (col_id, store) = store_with_collection();
    let (service, store_arc, publisher, published, responder) = service_with_publisher(store);

    let col = CollectionInterface::new(col_id.clone(), store_arc.clone(), publisher);
    let res = col.delete().await.unwrap();
    assert_eq!(res.as_str(), "/");

    assert!(!store_arc.read().await.collections.contains_key(&col_id));

    let section = published.lock().unwrap().last().unwrap().clone();
    assert!(!section.collections.contains_key(&col_id));

    drop(col);
    drop(service);
    responder.join().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mutation_reply_waits_for_exact_ack() {
    let (col_id, store) = store_with_collection();
    let store = Arc::new(RwLock::new(store));
    let (publisher, mut host) = controlled_publisher();
    let mut collection = CollectionInterface::new(col_id.clone(), store.clone(), publisher);
    let mutation = tokio::spawn(async move { collection.set_label("after".into()).await });

    let generation = read_publication(&mut host);
    assert_eq!(generation, 0);
    assert!(
        !mutation.is_finished(),
        "mutation replied before exact host ACK"
    );
    host.write_all(&encode_response(ResponseKind::Ack, generation))
        .unwrap();
    mutation.await.unwrap().unwrap();
    assert_eq!(
        store.read().await.get_collection_label(&col_id),
        Some("after")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mutation_reject_and_channel_loss_return_failed_without_rollback() {
    for reject in [true, false] {
        let (col_id, store) = store_with_collection();
        let store = Arc::new(RwLock::new(store));
        let (publisher, mut host) = controlled_publisher();
        let mut collection = CollectionInterface::new(col_id.clone(), store.clone(), publisher);
        let mutation = tokio::spawn(async move { collection.set_label("after".into()).await });

        let generation = read_publication(&mut host);
        assert!(!mutation.is_finished());
        if reject {
            host.write_all(&encode_response(ResponseKind::Reject, generation))
                .unwrap();
        } else {
            drop(host);
        }
        let result = mutation.await.unwrap();
        assert!(matches!(result, Err(fdo::Error::Failed(_))));
        assert_eq!(
            store.read().await.get_collection_label(&col_id),
            Some("after"),
            "current mutation semantics intentionally do not roll back"
        );
    }
}
