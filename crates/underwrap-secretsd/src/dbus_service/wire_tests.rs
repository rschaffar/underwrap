//! Actual D-Bus dispatch with simulated host publication ACK/REJECT responses.
use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use underwrap_secretsd_store::SecretStore;
use zbus::{OwnedObjectPath, OwnedVariant, Proxy, Str};

use super::{CollectionInterface, ItemInterface, Secret, alias_path, collection_path};
use crate::{
    BASE_PATH,
    test_support::{HostResponse, ScriptedPublisher, WireHarness},
};

mod contract;

const SERVICE: &str = "org.freedesktop.Secret.Service";
const COLLECTION: &str = "org.freedesktop.Secret.Collection";
const ITEM: &str = "org.freedesktop.Secret.Item";

fn secret() -> Secret {
    Secret {
        session: OwnedObjectPath::try_from("/").unwrap(),
        parameters: vec![],
        value: vec![],
        content_type: "text/plain".into(),
    }
}

async fn set_alias(service: &Proxy<'_>, name: &str, collection: &str) -> zbus::Result<()> {
    service
        .call(
            "SetAlias",
            &(name, OwnedObjectPath::try_from(collection).unwrap()),
        )
        .await
}

async fn create_collection(service: &Proxy<'_>, name: &str, alias: &str) -> OwnedObjectPath {
    let properties = HashMap::from([(
        "org.freedesktop.Secret.Collection.Label",
        OwnedVariant::from(Str::from(name)),
    )]);
    let (path, prompt): (OwnedObjectPath, OwnedObjectPath) = service
        .call("CreateCollection", &(properties, alias))
        .await
        .unwrap();
    assert_eq!(prompt.as_str(), "/");
    path
}

async fn create_item(collection: &Proxy<'_>) -> OwnedObjectPath {
    let properties: HashMap<String, OwnedVariant> = HashMap::new();
    let (path, prompt): (OwnedObjectPath, OwnedObjectPath) = collection
        .call("CreateItem", &(properties, secret(), false))
        .await
        .unwrap();
    assert_eq!(prompt.as_str(), "/");
    path
}

async fn delete(object: &Proxy<'_>) {
    let prompt: OwnedObjectPath = object.call("Delete", &()).await.unwrap();
    assert_eq!(prompt.as_str(), "/");
}

async fn assert_item_gone(bus: &WireHarness, path: &str) {
    assert!(
        bus.server
            .object_server()
            .interface::<_, ItemInterface>(path)
            .await
            .is_err()
    );
    let item = bus.proxy(path, ITEM).await;
    assert!(
        item.call::<_, _, ()>("SetSecret", &(secret(),))
            .await
            .is_err()
    );
    assert!(
        item.set_property("Attributes", HashMap::<String, String>::new())
            .await
            .is_err()
    );
    assert!(item.set_property("Label", "discarded").await.is_err());
}

async fn assert_collection_gone(bus: &WireHarness, path: &str) {
    assert!(
        bus.server
            .object_server()
            .interface::<_, CollectionInterface>(path)
            .await
            .is_err()
    );
    let collection = bus.proxy(path, COLLECTION).await;
    assert!(collection.get_property::<String>("Label").await.is_err());
    assert!(collection.set_property("Label", "discarded").await.is_err());
}

#[tokio::test]
async fn aliases_follow_accepted_state_on_wire() {
    let mut initial = SecretStore::new_default();
    initial.create_collection("C2", None);
    initial.set_alias("A", Some("default"));
    let c1 = initial.collections["default"].clone();
    let store = Arc::new(RwLock::new(initial));
    let script = ScriptedPublisher::new(vec![HostResponse::Ack; 8]);
    let bus = WireHarness::new(&store, &script.publisher()).await;
    let service = bus.proxy(BASE_PATH, SERVICE).await;
    let path = alias_path("A").unwrap();
    let alias = bus.proxy(path.as_str(), COLLECTION).await;
    assert_eq!(
        alias.get_property::<String>("Label").await.unwrap(),
        "Default"
    );
    let retained = bus
        .server
        .object_server()
        .interface::<_, CollectionInterface>(path.as_str())
        .await
        .unwrap();

    set_alias(&service, "A", collection_path("C2").unwrap().as_str())
        .await
        .unwrap();
    let read_alias: OwnedObjectPath = service.call("ReadAlias", &("A",)).await.unwrap();
    assert_eq!(read_alias.as_str(), collection_path("C2").unwrap().as_str());
    assert_eq!(alias.get_property::<String>("Label").await.unwrap(), "C2");
    alias.set_property("Label", "C2_updated").await.unwrap();
    let item = create_item(&alias).await;
    assert!(
        item.as_str()
            .starts_with(&format!("{BASE_PATH}/collection/C2/"))
    );
    let items: Vec<OwnedObjectPath> = alias.get_property("Items").await.unwrap();
    assert_eq!(items, vec![item.clone()]);
    {
        let state = store.read().await;
        assert!(
            state.collections["default"] == c1,
            "alias mutation changed C1"
        );
        assert_eq!(state.collections["C2"].label, "C2_updated");
        assert_eq!(state.collections["C2"].items.len(), 1);
    }

    set_alias(&service, "A", "/").await.unwrap();
    assert_collection_gone(&bus, path.as_str()).await;
    assert!(matches!(
        retained.get().await.set_label("discarded".into()).await,
        Err(zbus::fdo::Error::UnknownObject(_))
    ));
    let read_alias: OwnedObjectPath = service.call("ReadAlias", &("A",)).await.unwrap();
    assert_eq!(read_alias.as_str(), "/");
    set_alias(
        &service,
        "new_alias",
        collection_path("C2").unwrap().as_str(),
    )
    .await
    .unwrap();
    let new_path = alias_path("new_alias").unwrap();
    assert_eq!(
        bus.proxy(new_path.as_str(), COLLECTION)
            .await
            .get_property::<String>("Label")
            .await
            .unwrap(),
        "C2_updated"
    );
    let created = create_collection(&service, "C3", "created_alias").await;
    let created_alias = alias_path("created_alias").unwrap();
    assert_eq!(
        bus.proxy(created_alias.as_str(), COLLECTION)
            .await
            .get_property::<String>("Label")
            .await
            .unwrap(),
        "C3"
    );
    assert_eq!(created.as_str(), collection_path("C3").unwrap().as_str());
    for (name, expected) in [
        ("new_alias", collection_path("C2").unwrap()),
        ("created_alias", collection_path("C3").unwrap()),
    ] {
        let target: OwnedObjectPath = service.call("ReadAlias", &(name,)).await.unwrap();
        assert_eq!(target.as_str(), expected.as_str());
    }
    // Delete through an alias retargeted after its export: never delete its
    // previous collection, and remove the new target's other dependent alias.
    set_alias(&service, "new_alias", created.as_str())
        .await
        .unwrap();
    delete(&bus.proxy(new_path.as_str(), COLLECTION).await).await;
    assert_collection_gone(&bus, created.as_str()).await;
    assert_collection_gone(&bus, created_alias.as_str()).await;
    assert_collection_gone(&bus, new_path.as_str()).await;
    assert!(store.read().await.collections.contains_key("C2"));
    assert!(store.read().await.collections["default"] == c1);
    drop((alias, service));
    bus.close().await;
    assert_eq!(script.finish().len(), 8);
}

#[tokio::test]
async fn accepted_deletions_remove_dispatch_and_retained_handlers_reject_setters() {
    let mut initial = SecretStore::new_default();
    let first = initial
        .create_item("default", "", &HashMap::new(), b"", "text/plain", false)
        .unwrap()
        .0;
    let second = initial
        .create_item("default", "", &HashMap::new(), b"", "text/plain", false)
        .unwrap()
        .0;
    initial.set_alias("A", Some("default"));
    let store = Arc::new(RwLock::new(initial));
    let script = ScriptedPublisher::new(vec![HostResponse::Ack; 2]);
    let bus = WireHarness::new(&store, &script.publisher()).await;
    let collection_path = collection_path("default").unwrap();
    let alias_path = alias_path("A").unwrap();
    let paths = [first, second].map(|id| format!("{collection_path}/{id}"));
    let retained_first = bus
        .server
        .object_server()
        .interface::<_, ItemInterface>(paths[0].as_str())
        .await
        .unwrap();
    let retained_second = bus
        .server
        .object_server()
        .interface::<_, ItemInterface>(paths[1].as_str())
        .await
        .unwrap();
    let retained_collection = bus
        .server
        .object_server()
        .interface::<_, CollectionInterface>(collection_path.as_str())
        .await
        .unwrap();
    delete(&bus.proxy(&paths[0], ITEM).await).await;
    assert_item_gone(&bus, &paths[0]).await;
    assert_retained_item_absent(&retained_first).await;
    let after_item = store.read().await.clone();
    assert!(after_item.collections["default"].items.len() == 1);

    // Deletion via an alias resolves the accepted collection and removes every
    // descendant and every dependent alias, including the startup default alias.
    delete(&bus.proxy(alias_path.as_str(), COLLECTION).await).await;
    assert_item_gone(&bus, &paths[1]).await;
    assert_retained_item_absent(&retained_second).await;
    assert_collection_gone(&bus, collection_path.as_str()).await;
    assert_collection_gone(&bus, alias_path.as_str()).await;
    assert_collection_gone(&bus, super::alias_path("default").unwrap().as_str()).await;
    let before = store.read().await.clone();
    assert!(matches!(
        retained_collection
            .get()
            .await
            .set_label("discarded".into())
            .await,
        Err(zbus::fdo::Error::UnknownObject(_))
    ));
    assert!(store.read().await.collections.is_empty());
    assert!(store.read().await.aliases.is_empty());
    assert!(
        *store.read().await == before,
        "missing-object setters changed accepted state"
    );
    bus.close().await;
    assert_eq!(
        script.finish().len(),
        2,
        "missing objects must not publish no-op candidates"
    );
}

async fn assert_retained_item_absent(item: &zbus::object_server::InterfaceRef<ItemInterface>) {
    let handler = item.get().await;
    assert!(matches!(
        handler.set_secret(secret()).await,
        Err(zbus::fdo::Error::UnknownObject(_))
    ));
    assert!(matches!(
        handler.set_attributes(HashMap::new()).await,
        Err(zbus::fdo::Error::UnknownObject(_))
    ));
    assert!(matches!(
        handler.set_label("discarded".into()).await,
        Err(zbus::fdo::Error::UnknownObject(_))
    ));
}

#[tokio::test]
async fn bounded_create_delete_cycles_leave_no_obsolete_registry_nodes() {
    let store = Arc::new(RwLock::new(SecretStore::new_default()));
    let script = ScriptedPublisher::new(vec![HostResponse::Ack; 24]);
    let bus = WireHarness::new(&store, &script.publisher()).await;
    let service = bus.proxy(BASE_PATH, SERVICE).await;
    for cycle in 0..6 {
        // Recreate the same names as well as replacing random descendant paths.
        let n = cycle % 2;
        let path = create_collection(&service, &format!("cycle_{n}"), &format!("alias_{n}")).await;
        let collection = bus.proxy(path.as_str(), COLLECTION).await;
        let item = create_item(&collection).await;
        delete(&bus.proxy(item.as_str(), ITEM).await).await;
        assert_item_gone(&bus, item.as_str()).await;
        // Introspect the actual registry, not only the bounded canonical state.
        let xml: String = bus
            .proxy(path.as_str(), "org.freedesktop.DBus.Introspectable")
            .await
            .call("Introspect", &())
            .await
            .unwrap();
        assert!(!xml.contains("<node name="), "deleted item node retained");
        delete(&collection).await;
        assert_collection_gone(&bus, path.as_str()).await;
        assert_collection_gone(&bus, alias_path(&format!("alias_{n}")).unwrap().as_str()).await;
    }
    for (container, expected) in [("collection", 1), ("aliases", 1)] {
        let path = format!("{BASE_PATH}/{container}");
        let xml: String = bus
            .proxy(&path, "org.freedesktop.DBus.Introspectable")
            .await
            .call("Introspect", &())
            .await
            .unwrap();
        assert_eq!(
            xml.matches("<node name=").count(),
            expected,
            "obsolete export nodes retained"
        );
    }
    assert_eq!(store.read().await.collections.len(), 1);
    drop(service);
    bus.close().await;
    assert_eq!(script.finish().len(), 24);
}

mod concurrency;
mod rollback;
