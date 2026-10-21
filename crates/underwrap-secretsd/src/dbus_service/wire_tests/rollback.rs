use super::*;

#[tokio::test]
async fn rejected_alias_and_delete_changes_preserve_callable_graph_and_state() {
    let mut initial = SecretStore::new_default();
    initial.create_collection("C2", None);
    initial.set_alias("A", Some("default"));
    let item_id = initial
        .create_item("default", "item", &HashMap::new(), b"", "text/plain", false)
        .unwrap()
        .0;
    let before = initial.clone();
    let store = Arc::new(RwLock::new(initial));
    let script = ScriptedPublisher::new(vec![HostResponse::Reject; 6]);
    let bus = WireHarness::new(&store, &script.publisher()).await;
    let service = bus.proxy(BASE_PATH, SERVICE).await;
    let alias_path = alias_path("A").unwrap();
    let alias = bus.proxy(alias_path.as_str(), COLLECTION).await;
    for (name, target) in [
        ("A", collection_path("C2").unwrap().to_string()),
        ("A", "/".into()),
        ("new_alias", collection_path("C2").unwrap().to_string()),
    ] {
        assert!(set_alias(&service, name, &target).await.is_err());
        assert!(
            *store.read().await == before,
            "rejected alias changed accepted state"
        );
        assert_eq!(
            alias.get_property::<String>("Label").await.unwrap(),
            "Default"
        );
        assert_collection_gone(
            &bus,
            super::super::alias_path("new_alias").unwrap().as_str(),
        )
        .await;
    }
    let properties = HashMap::from([(
        "org.freedesktop.Secret.Collection.Label",
        OwnedValue::from(Str::from("rejected")),
    )]);
    assert!(
        service
            .call::<_, _, (OwnedObjectPath, OwnedObjectPath)>(
                "CreateCollection",
                &(properties, "rejected_alias")
            )
            .await
            .is_err()
    );
    assert_collection_gone(&bus, collection_path("rejected").unwrap().as_str()).await;
    assert_collection_gone(
        &bus,
        super::super::alias_path("rejected_alias").unwrap().as_str(),
    )
    .await;
    let item_path = format!("{BASE_PATH}/collection/default/{item_id}");
    let item = bus.proxy(&item_path, ITEM).await;
    assert!(
        item.call::<_, _, OwnedObjectPath>("Delete", &())
            .await
            .is_err()
    );
    assert_eq!(item.get_property::<String>("Label").await.unwrap(), "item");
    let _: (Secret,) = item
        .call("GetSecret", &(OwnedObjectPath::try_from("/").unwrap(),))
        .await
        .unwrap();
    assert!(
        alias
            .call::<_, _, OwnedObjectPath>("Delete", &())
            .await
            .is_err()
    );
    assert_eq!(
        alias.get_property::<String>("Label").await.unwrap(),
        "Default"
    );
    assert!(
        bus.server
            .object_server()
            .interface::<_, ItemInterface>(item_path.as_str())
            .await
            .is_ok()
    );
    assert!(
        *store.read().await == before,
        "rejected deletion changed accepted state"
    );
    drop((service, alias, item));
    bus.close().await;
    assert_eq!(script.finish().len(), 6);
}

#[tokio::test]
async fn transport_failures_preserve_state_and_exports() {
    for response in [HostResponse::Eof, HostResponse::WrongGeneration] {
        for operation in [
            "retarget",
            "clear",
            "create_alias",
            "item_delete",
            "collection_delete",
        ] {
            let mut initial = SecretStore::new_default();
            initial.create_collection("C2", None);
            initial.set_alias("A", Some("default"));
            let item_id = initial
                .create_item("default", "item", &HashMap::new(), b"", "text/plain", false)
                .unwrap()
                .0;
            let before = initial.clone();
            let store = Arc::new(RwLock::new(initial));
            let script = ScriptedPublisher::new(vec![response]);
            let bus = WireHarness::new(&store, &script.publisher()).await;
            let service = bus.proxy(BASE_PATH, SERVICE).await;
            let item_path = format!("{BASE_PATH}/collection/default/{item_id}");
            let alias_path = alias_path("A").unwrap();
            let alias = bus.proxy(alias_path.as_str(), COLLECTION).await;
            match operation {
                "retarget" => assert!(
                    set_alias(&service, "A", collection_path("C2").unwrap().as_str())
                        .await
                        .is_err()
                ),
                "clear" => assert!(set_alias(&service, "A", "/").await.is_err()),
                "create_alias" => assert!(
                    set_alias(&service, "new", collection_path("C2").unwrap().as_str())
                        .await
                        .is_err()
                ),
                "item_delete" => assert!(
                    bus.proxy(&item_path, ITEM)
                        .await
                        .call::<_, _, OwnedObjectPath>("Delete", &())
                        .await
                        .is_err()
                ),
                "collection_delete" => assert!(
                    alias
                        .call::<_, _, OwnedObjectPath>("Delete", &())
                        .await
                        .is_err()
                ),
                _ => unreachable!(),
            }
            assert!(
                *store.read().await == before,
                "transport failure changed accepted state"
            );
            assert_eq!(
                alias.get_property::<String>("Label").await.unwrap(),
                "Default"
            );
            assert_eq!(
                bus.proxy(&item_path, ITEM)
                    .await
                    .get_property::<String>("Label")
                    .await
                    .unwrap(),
                "item"
            );
            assert_collection_gone(&bus, super::super::alias_path("new").unwrap().as_str()).await;
            drop((alias, service));
            bus.close().await;
            assert_eq!(script.finish().len(), 1);
        }
    }
}

#[tokio::test]
async fn extant_object_setters_still_publish_ack_and_reject_on_wire() {
    let mut initial = SecretStore::new_default();
    let id = initial
        .create_item(
            "default",
            "before",
            &HashMap::new(),
            b"",
            "text/plain",
            false,
        )
        .unwrap()
        .0;
    let store = Arc::new(RwLock::new(initial));
    let script = ScriptedPublisher::new(vec![
        HostResponse::Reject,
        HostResponse::Ack,
        HostResponse::Reject,
        HostResponse::Ack,
        HostResponse::Reject,
        HostResponse::Ack,
        HostResponse::Reject,
        HostResponse::Ack,
    ]);
    let bus = WireHarness::new(&store, &script.publisher()).await;
    let item_path = format!("{BASE_PATH}/collection/default/{id}");
    let item = bus.proxy(&item_path, ITEM).await;
    for operation in ["secret", "attributes", "item_label", "collection_label"] {
        let before = store.read().await.clone();
        for accepted in [false, true] {
            let result = match operation {
                "secret" => item
                    .call::<_, _, ()>(
                        "SetSecret",
                        &({
                            let mut value = secret();
                            value.content_type = "application/test".into();
                            value
                        },),
                    )
                    .await
                    .is_ok(),
                "attributes" => item
                    .set_property("Attributes", HashMap::from([("key", "value")]))
                    .await
                    .is_ok(),
                "item_label" => item.set_property("Label", "after").await.is_ok(),
                "collection_label" => bus
                    .proxy(collection_path("default").unwrap().as_str(), COLLECTION)
                    .await
                    .set_property("Label", "after")
                    .await
                    .is_ok(),
                _ => unreachable!(),
            };
            assert_eq!(result, accepted);
            if !accepted {
                assert!(
                    *store.read().await == before,
                    "rejected setter changed accepted state"
                );
            }
        }
        let state = store.read().await;
        let collection = &state.collections["default"];
        let item = &collection.items[&id];
        match operation {
            "secret" => assert_eq!(item.content_type, "application/test"),
            "attributes" => assert_eq!(
                item.attributes.get("key").map(String::as_str),
                Some("value")
            ),
            "item_label" => assert_eq!(item.label, "after"),
            "collection_label" => assert_eq!(collection.label, "after"),
            _ => unreachable!(),
        }
    }
    drop(item);
    bus.close().await;
    assert_eq!(script.finish().len(), 8);
}
