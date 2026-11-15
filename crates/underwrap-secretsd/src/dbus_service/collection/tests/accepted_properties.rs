use super::*;

fn assert_one_publication(published: &[ObservedPublication], accepted: &SecretStore) {
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].generation, 0);
    assert_eq!(published[0].section, section_from_store(accepted));
}

#[derive(Clone, Copy, Debug)]
enum AcceptedNewProperties {
    BothAbsent,
    LabelAbsent,
    AttributesAbsent,
    UnknownIgnored,
}

#[tokio::test]
async fn accepted_optional_item_properties_publish_exact_defaults_and_metadata() {
    for row in [
        AcceptedNewProperties::BothAbsent,
        AcceptedNewProperties::LabelAbsent,
        AcceptedNewProperties::AttributesAbsent,
        AcceptedNewProperties::UnknownIgnored,
    ] {
        let (properties, expected_label, expected_attributes) = match row {
            AcceptedNewProperties::BothAbsent => (HashMap::new(), "", HashMap::new()),
            AcceptedNewProperties::LabelAbsent => (
                HashMap::from([(
                    ITEM_ATTRIBUTES_PROPERTY.to_string(),
                    string_dict(&[("account", "alice")]),
                )]),
                "",
                HashMap::from([("account".into(), "alice".into())]),
            ),
            AcceptedNewProperties::AttributesAbsent => (
                HashMap::from([(
                    ITEM_LABEL_PROPERTY.to_string(),
                    Variant::from("label-only".to_string()),
                )]),
                "label-only",
                HashMap::new(),
            ),
            AcceptedNewProperties::UnknownIgnored => (
                HashMap::from([(
                    "org.example.Unknown".to_string(),
                    Variant::Variant(Box::new(Variant::from("ignored".to_string()))),
                )]),
                "",
                HashMap::new(),
            ),
        };
        let initial = store_with_collection();
        let (service, store, script) = service_with_script(initial, vec![HostResponse::Ack]);
        let collection = CollectionInterface::new("Test".into(), store.clone(), script.publisher());
        let object_server = ObjectServerHarness::new().await;
        let request_secret = format!("accepted-new-{row:?}");

        let result = {
            let server = object_server.object_server();
            collection
                .create_item(
                    properties,
                    secret(request_secret.as_bytes()),
                    false,
                    &server,
                )
                .await
                .unwrap()
        };
        let path = result.0.as_str().to_string();
        let accepted = store.read().await.clone();
        let published = script.finish();
        let item = accepted.collections["Test"].items.values().next().unwrap();

        assert_eq!(result.1.as_str(), "/", "{row:?}");
        assert!(object_server.has_item(&path).await, "{row:?}");
        assert_eq!(accepted.collections["Test"].items.len(), 1, "{row:?}");
        assert_eq!(item.label, expected_label, "{row:?}");
        assert_eq!(item.attributes, expected_attributes, "{row:?}");
        assert_eq!(item.secret, request_secret.as_bytes(), "{row:?}");
        assert_eq!(item.content_type, "application/octet-stream", "{row:?}");
        assert_one_publication(&published, &accepted);
        object_server.close().await;
        drop(service);
    }
}

#[tokio::test]
async fn matching_and_absent_attributes_replace_by_subset_without_metadata_loss() {
    for absent_attributes in [false, true] {
        let mut initial = store_with_collection();
        let existing_attributes = HashMap::from([
            ("account".into(), "alice".into()),
            ("retained".into(), "strict-superset".into()),
        ]);
        let (item_id, _) = initial
            .create_item(
                "Test",
                "old-label",
                &existing_attributes,
                b"accepted-old-secret",
                "text/old",
                false,
            )
            .unwrap();
        let (service, store, script) = service_with_script(initial, vec![HostResponse::Ack]);
        let collection = CollectionInterface::new("Test".into(), store.clone(), script.publisher());
        let object_server = ObjectServerHarness::new().await;
        let existing_path = format!("{BASE_PATH}/collection/Test/{item_id}");
        {
            let server = object_server.object_server();
            server
                .at(
                    existing_path.as_str(),
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
        let mut properties = item_properties("replacement-label");
        if !absent_attributes {
            properties.insert(
                ITEM_ATTRIBUTES_PROPERTY.to_string(),
                string_dict(&[("account", "alice")]),
            );
        }

        let result = {
            let server = object_server.object_server();
            collection
                .create_item(
                    properties,
                    secret(b"accepted-replacement-secret"),
                    true,
                    &server,
                )
                .await
                .unwrap()
        };
        let accepted = store.read().await.clone();
        let published = script.finish();
        let item = &accepted.collections["Test"].items[&item_id];

        assert_eq!(result.0.as_str(), existing_path, "{absent_attributes}");
        assert_eq!(accepted.collections["Test"].items.len(), 1);
        assert!(object_server.has_item(&existing_path).await);
        assert_eq!(item.attributes, existing_attributes);
        assert_eq!(item.label, "replacement-label");
        assert_eq!(item.secret, b"accepted-replacement-secret");
        assert_eq!(item.content_type, "application/octet-stream");
        assert_one_publication(&published, &accepted);
        object_server.close().await;
        drop(service);
    }
}

#[tokio::test]
async fn nonmatching_attributes_create_and_register_new_item_without_changing_old_item() {
    let mut initial = store_with_collection();
    let old_attributes = HashMap::from([("account".into(), "alice".into())]);
    let (old_id, _) = initial
        .create_item(
            "Test",
            "old-label",
            &old_attributes,
            b"accepted-old-secret",
            "text/old",
            false,
        )
        .unwrap();
    let old_item = initial.collections["Test"].items[&old_id].clone();
    let (service, store, script) = service_with_script(initial, vec![HostResponse::Ack]);
    let collection = CollectionInterface::new("Test".into(), store.clone(), script.publisher());
    let object_server = ObjectServerHarness::new().await;
    let old_path = format!("{BASE_PATH}/collection/Test/{old_id}");
    {
        let server = object_server.object_server();
        server
            .at(
                old_path.as_str(),
                ItemInterface::new(
                    old_id.clone(),
                    "Test".into(),
                    store.clone(),
                    script.publisher(),
                ),
            )
            .await
            .unwrap();
    }
    let requested_attributes = HashMap::from([("account".into(), "bob".into())]);
    let properties = HashMap::from([
        (
            ITEM_LABEL_PROPERTY.to_string(),
            Variant::from("new-label".to_string()),
        ),
        (
            ITEM_ATTRIBUTES_PROPERTY.to_string(),
            string_dict(&[("account", "bob")]),
        ),
    ]);

    let result = {
        let server = object_server.object_server();
        collection
            .create_item(properties, secret(b"accepted-new-secret"), true, &server)
            .await
            .unwrap()
    };
    let new_path = result.0.as_str().to_string();
    let new_id = new_path.rsplit('/').next().unwrap();
    let accepted = store.read().await.clone();
    let published = script.finish();
    let new_item = &accepted.collections["Test"].items[new_id];

    assert_ne!(new_path, old_path);
    assert_eq!(accepted.collections["Test"].items.len(), 2);
    assert_eq!(accepted.collections["Test"].items[&old_id], old_item);
    assert!(object_server.has_item(&old_path).await);
    assert!(object_server.has_item(&new_path).await);
    assert_eq!(new_item.label, "new-label");
    assert_eq!(new_item.attributes, requested_attributes);
    assert_eq!(new_item.secret, b"accepted-new-secret");
    assert_eq!(new_item.content_type, "application/octet-stream");
    assert_one_publication(&published, &accepted);
    object_server.close().await;
    drop(service);
}
