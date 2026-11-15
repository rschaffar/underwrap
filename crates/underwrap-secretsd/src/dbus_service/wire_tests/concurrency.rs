use super::*;
use tokio::sync::Notify;

struct RegistryReader {
    entered: Arc<Notify>,
    release: Arc<Notify>,
    store: Arc<RwLock<SecretStore>>,
}

#[zbus::interface(name = "org.underwrap.Test.RegistryReader")]
impl RegistryReader {
    #[zbus(property)]
    async fn label(&self) -> String {
        // Properties.Get retains the registry read lock throughout the handler.
        self.entered.notify_one();
        self.release.notified().await;
        self.store.read().await.collections["default"].label.clone()
    }
}

async fn overlap_property_and_graph_change(cancel: bool) {
    let mut initial = SecretStore::new_default();
    let id = initial
        .create_item("default", "", &HashMap::new(), b"", "text/plain", false)
        .unwrap()
        .0;
    let store = Arc::new(RwLock::new(initial));
    let script = ScriptedPublisher::new(vec![HostResponse::Ack; if cancel { 4 } else { 5 }]);
    let bus = WireHarness::new(&store, &script.publisher()).await;
    // Graph methods retain shared interface guards. Property setters must not
    // require exclusive interface guards while holding the registry read lock.
    let path = collection_path("default").unwrap();
    let collection_ref = bus
        .server
        .object_server()
        .interface::<_, CollectionInterface>(path.as_str())
        .await
        .unwrap();
    let collection_guard = collection_ref.get().await;
    bus.proxy(path.as_str(), COLLECTION)
        .await
        .set_property("Label", "content_label")
        .await
        .unwrap();
    let item_path = format!("{path}/{id}");
    let item_ref = bus
        .server
        .object_server()
        .interface::<_, ItemInterface>(item_path.as_str())
        .await
        .unwrap();
    let item_guard = item_ref.get().await;
    let item = bus.proxy(&item_path, ITEM).await;
    item.set_property("Label", "content_item").await.unwrap();
    item.set_property("Attributes", HashMap::<String, String>::new())
        .await
        .unwrap();
    drop((collection_guard, item_guard, item));
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    bus.server
        .object_server()
        .at(
            "/registry_reader",
            RegistryReader {
                entered: entered.clone(),
                release: release.clone(),
                store: store.clone(),
            },
        )
        .await
        .unwrap();
    let client = bus.client.clone();
    let destination = bus
        .server
        .unique_name()
        .map(|name| name.as_str())
        .unwrap_or(":1.0")
        .to_owned();
    let property = tokio::spawn(async move {
        let reader: Proxy<'_> = zbus::proxy::Builder::new(&client)
            .destination(destination)
            .path("/registry_reader")
            .interface("org.underwrap.Test.RegistryReader")
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .unwrap();
        reader.get_property::<String>("Label").await.unwrap()
    });
    entered.notified().await;
    let prepared = Arc::new(Notify::new());
    let mutation = {
        let store = store.clone();
        let publisher = script.publisher();
        let server = bus.server.clone();
        let prepared = prepared.clone();
        tokio::spawn(async move {
            super::super::mutation::mutate_and_export(
                &store,
                &publisher,
                server.object_server(),
                |candidate| {
                    candidate.create_collection("pending", None);
                    prepared.notify_one();
                    Ok(())
                },
            )
            .await
            .unwrap();
        })
    };
    prepared.notified().await;
    // The read must acquire after the write-guarded candidate/ACK/live commit,
    // even while registry reconciliation remains blocked by Properties.Get.
    assert!(store.read().await.collections.contains_key("pending"));
    assert!(!mutation.is_finished());
    if cancel {
        mutation.abort();
        let _ = mutation.await;
        panic!("cancelled accepted reconciliation did not stop the helper");
    }
    // Poll a second graph method into its pending gate acquisition, rather
    // than infer exclusion from a spawned task that might not have run yet.
    let publisher = script.publisher();
    let server = bus.server.object_server();
    let mut next = Box::pin(super::super::mutation::mutate_and_export(
        &store,
        &publisher,
        server,
        |candidate| {
            candidate.set_alias("later", Some("pending"));
            Ok(())
        },
    ));
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(next.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    assert!(!store.read().await.aliases.contains_key("later"));
    release.notify_one();
    assert_eq!(property.await.unwrap(), "content_label");
    mutation.await.unwrap();
    next.await.unwrap();
    assert_eq!(
        bus.proxy(collection_path("pending").unwrap().as_str(), COLLECTION)
            .await
            .get_property::<String>("Label")
            .await
            .unwrap(),
        "pending"
    );
    assert_eq!(
        bus.proxy(alias_path("later").unwrap().as_str(), COLLECTION)
            .await
            .get_property::<String>("Label")
            .await
            .unwrap(),
        "pending"
    );
    bus.close().await;
    assert_eq!(script.finish().len(), 5);
}

#[test]
fn graph_reconciliation_avoids_property_lock_inversion_and_cancellation_fail_stops() {
    const CHILD: &str = "UNDERWRAP_TEST_GRAPH_OVERLAP_CHILD";
    if let Ok(mode) = std::env::var(CHILD) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(overlap_property_and_graph_change(mode == "cancel"));
        return;
    }
    for (mode, expected) in [("complete", 0), ("cancel", 1)] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "dbus_service::wire_tests::concurrency::graph_reconciliation_avoids_property_lock_inversion_and_cancellation_fail_stops", "--nocapture"])
            .env(CHILD, mode).output().unwrap();
        assert_eq!(output.status.code(), Some(expected));
        if mode == "cancel" {
            assert!(
                String::from_utf8(output.stderr)
                    .unwrap()
                    .contains("accepted state export reconciliation failed; stopping")
            );
        }
    }
}
