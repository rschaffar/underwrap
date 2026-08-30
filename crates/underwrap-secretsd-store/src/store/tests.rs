use super::*;
use std::collections::HashMap;

const ITEM_MODIFIED_BASELINE: u64 = 11;
const COLLECTION_MODIFIED_BASELINE: u64 = 17;

fn store_with_backdated_item() -> (SecretStore, String, String) {
    let mut store = SecretStore::new_default();
    let target_id = store
        .create_item(
            "default",
            "original label",
            &HashMap::from([("original key".into(), "original value".into())]),
            b"synthetic-original-secret",
            "text/plain",
            false,
        )
        .unwrap()
        .0;
    let unrelated_id = store
        .create_item(
            "default",
            "unrelated label",
            &HashMap::from([("unrelated key".into(), "unrelated value".into())]),
            b"synthetic-unrelated-secret",
            "application/octet-stream",
            false,
        )
        .unwrap()
        .0;
    let collection = store.collections.get_mut("default").unwrap();
    collection.modified = COLLECTION_MODIFIED_BASELINE;
    collection.items.get_mut(&target_id).unwrap().modified = ITEM_MODIFIED_BASELINE;
    (store, target_id, unrelated_id)
}

fn assert_item_mutation_metadata(
    before: &SecretStore,
    after: &SecretStore,
    target_id: &str,
    unrelated_id: &str,
) {
    let before_collection = &before.collections["default"];
    let after_collection = &after.collections["default"];
    let before_item = &before_collection.items[target_id];
    let after_item = &after_collection.items[target_id];

    assert_eq!(after_item.modified, after_collection.modified);
    assert!(after_item.modified > ITEM_MODIFIED_BASELINE);
    assert!(after_collection.modified > COLLECTION_MODIFIED_BASELINE);
    assert_eq!(after_item.created, before_item.created);
    assert_eq!(after_collection.created, before_collection.created);
    assert_eq!(after_collection.label, before_collection.label);
    assert_eq!(
        after_collection.items[unrelated_id],
        before_collection.items[unrelated_id]
    );
    assert_eq!(after.aliases, before.aliases);
}

#[test]
fn new_default_has_default_collection() {
    let store = SecretStore::new_default();
    assert!(store.collections.contains_key("default"));
    assert_eq!(store.get_alias("default"), Some("default"));
    assert_eq!(store.get_collection_label("default"), Some("Default"));
}

#[test]
fn create_collection_id_is_the_name() {
    let mut store = SecretStore::new_default();
    let id = store.create_collection("gogcli", Some("mine"));
    // The id is the requested name, not an opaque counter, so clients
    // that locate the collection by object path find it.
    assert_eq!(id, "gogcli");
    assert!(store.collections.contains_key("gogcli"));
    assert_eq!(store.get_alias("mine"), Some("gogcli"));
    assert_eq!(store.get_collection_label("gogcli"), Some("gogcli"));
}

#[test]
fn create_collection_is_idempotent_by_name() {
    let mut store = SecretStore::new_default();
    let id1 = store.create_collection("gogcli", None);
    let id2 = store.create_collection("gogcli", None);
    assert_eq!(id1, "gogcli");
    assert_eq!(id1, id2, "a second create with the same name is a no-op");
    // Exactly one gogcli collection — no per-call litter.
    assert_eq!(store.collections.len(), 2); // default + gogcli
}

#[test]
fn create_collection_existing_alias_returns_existing() {
    let mut store = SecretStore::new_default();
    let id1 = store.create_collection("First", Some("myalias"));
    let id2 = store.create_collection("Second", Some("myalias"));
    assert_eq!(id1, id2, "existing alias should return existing collection");
}

#[test]
fn delete_collection_removes_aliases() {
    let mut store = SecretStore::new_default();
    let id = store.create_collection("Temp", Some("tempalias"));
    assert!(store.delete_collection(&id));
    assert_eq!(store.get_alias("tempalias"), None);
    assert!(!store.collections.contains_key(&id));
}

#[test]
fn delete_nonexistent_collection() {
    let mut store = SecretStore::new_default();
    assert!(!store.delete_collection("nonexistent"));
}

#[test]
fn create_and_get_item() {
    let mut store = SecretStore::new_default();
    let mut attrs = HashMap::new();
    attrs.insert("key".to_string(), "val".to_string());

    let (id, _) = store
        .create_item("default", "Test", &attrs, b"secret", "text/plain", false)
        .unwrap();

    assert_eq!(store.get_secret("default", &id), Some(b"secret".as_slice()));
    assert_eq!(store.get_label("default", &id), Some("Test"));
    assert_eq!(
        store.collections["default"].items[&id].content_type,
        "text/plain"
    );
    assert!(store.get_created("default", &id).is_some());
    assert!(store.get_modified("default", &id).is_some());
}

#[test]
fn item_ids_are_random_hex() {
    let mut store = SecretStore::new_default();
    let (id1, _) = store
        .create_item("default", "A", &HashMap::new(), b"a", "text/plain", false)
        .unwrap();
    let (id2, _) = store
        .create_item("default", "B", &HashMap::new(), b"b", "text/plain", false)
        .unwrap();

    // 32 lowercase hex chars (16 random bytes), a valid D-Bus path segment.
    for id in [&id1, &id2] {
        assert_eq!(id.len(), 32, "id should be 16 bytes hex-encoded: {id}");
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "id should be lowercase hex: {id}"
        );
    }
    // Two creates with the same (empty) attributes still get distinct ids.
    assert_ne!(id1, id2);
}

#[test]
fn create_item_in_nonexistent_collection_fails() {
    let mut store = SecretStore::new_default();
    let result = store.create_item(
        "nonexistent",
        "Test",
        &HashMap::new(),
        b"x",
        "text/plain",
        false,
    );
    assert!(result.is_err());
}

#[test]
fn replace_item_by_attributes() {
    let mut store = SecretStore::new_default();
    let mut attrs = HashMap::new();
    attrs.insert("service".to_string(), "gh".to_string());

    let (id1, replaced1) = store
        .create_item(
            "default",
            "Original",
            &attrs,
            b"old_secret",
            "text/plain",
            true,
        )
        .unwrap();
    assert!(!replaced1);

    let (id2, replaced2) = store
        .create_item(
            "default",
            "Updated",
            &attrs,
            b"new_secret",
            "text/plain",
            true,
        )
        .unwrap();
    assert!(replaced2);
    assert_eq!(id1, id2, "replace should reuse the same item ID");
    assert_eq!(
        store.get_secret("default", &id1),
        Some(b"new_secret".as_slice())
    );
    assert_eq!(store.get_label("default", &id1), Some("Updated"));
}

#[test]
fn no_replace_creates_new_item() {
    let mut store = SecretStore::new_default();
    let mut attrs = HashMap::new();
    attrs.insert("service".to_string(), "gh".to_string());

    let (id1, _) = store
        .create_item("default", "First", &attrs, b"secret1", "text/plain", false)
        .unwrap();
    let (id2, replaced) = store
        .create_item("default", "Second", &attrs, b"secret2", "text/plain", false)
        .unwrap();
    assert!(!replaced);
    assert_ne!(id1, id2);
}

#[test]
fn delete_item() {
    let mut store = SecretStore::new_default();
    let (id, _) = store
        .create_item(
            "default",
            "Test",
            &HashMap::new(),
            b"secret",
            "text/plain",
            false,
        )
        .unwrap();
    assert!(store.delete_item("default", &id));
    assert_eq!(store.get_secret("default", &id), None);
    assert!(!store.delete_item("default", &id));
}

#[test]
fn search_by_attributes() {
    let mut store = SecretStore::new_default();

    let mut attrs_gh = HashMap::new();
    attrs_gh.insert("service".to_string(), "gh".to_string());
    attrs_gh.insert("account".to_string(), "user1".to_string());

    let mut attrs_slack = HashMap::new();
    attrs_slack.insert("service".to_string(), "slack".to_string());

    let (id_gh, _) = store
        .create_item("default", "GH", &attrs_gh, b"gh_tok", "text/plain", false)
        .unwrap();
    let (_id_slack, _) = store
        .create_item(
            "default",
            "Slack",
            &attrs_slack,
            b"slack_tok",
            "text/plain",
            false,
        )
        .unwrap();

    let mut search = HashMap::new();
    search.insert("service".to_string(), "gh".to_string());
    let results = store.search("default", &search);
    assert_eq!(results, vec![id_gh.as_str()]);

    search.insert("account".to_string(), "user1".to_string());
    let results = store.search("default", &search);
    assert_eq!(results, vec![id_gh.as_str()]);

    let mut search_none = HashMap::new();
    search_none.insert("service".to_string(), "nonexistent".to_string());
    let results = store.search("default", &search_none);
    assert!(results.is_empty());
}

#[test]
fn search_empty_attributes_matches_all() {
    let mut store = SecretStore::new_default();
    let mut attrs = HashMap::new();
    attrs.insert("key".to_string(), "val".to_string());
    store
        .create_item("default", "A", &attrs, b"a", "text/plain", false)
        .unwrap();
    store
        .create_item("default", "B", &HashMap::new(), b"b", "text/plain", false)
        .unwrap();

    let results = store.search("default", &HashMap::new());
    assert_eq!(results.len(), 2);
}

#[test]
fn search_all_collections() {
    let mut store = SecretStore::new_default();
    let col2 = store.create_collection("Second", None);

    let mut attrs = HashMap::new();
    attrs.insert("service".to_string(), "gh".to_string());

    store
        .create_item("default", "GH1", &attrs, b"tok1", "text/plain", false)
        .unwrap();
    store
        .create_item(&col2, "GH2", &attrs, b"tok2", "text/plain", false)
        .unwrap();

    let results = store.search_all(&attrs);
    assert_eq!(results.len(), 2);
}

#[test]
fn set_secret_updates_fields_and_shared_modified_timestamp() {
    let (mut store, target_id, unrelated_id) = store_with_backdated_item();
    let before = store.clone();

    assert!(store.set_secret(
        "default",
        &target_id,
        b"synthetic-updated-secret",
        "application/octet-stream"
    ));

    let item = &store.collections["default"].items[&target_id];
    let before_item = &before.collections["default"].items[&target_id];
    assert_eq!(item.secret, b"synthetic-updated-secret");
    assert_eq!(item.content_type, "application/octet-stream");
    assert_eq!(item.attributes, before_item.attributes);
    assert_eq!(item.label, before_item.label);
    assert_item_mutation_metadata(&before, &store, &target_id, &unrelated_id);
}

#[test]
fn set_attributes_updates_field_and_shared_modified_timestamp() {
    let (mut store, target_id, unrelated_id) = store_with_backdated_item();
    let before = store.clone();
    let attributes = HashMap::from([("updated key".into(), "updated value".into())]);

    assert!(store.set_attributes("default", &target_id, attributes.clone()));

    let item = &store.collections["default"].items[&target_id];
    let before_item = &before.collections["default"].items[&target_id];
    assert_eq!(item.attributes, attributes);
    assert_eq!(item.secret, before_item.secret);
    assert_eq!(item.content_type, before_item.content_type);
    assert_eq!(item.label, before_item.label);
    assert_item_mutation_metadata(&before, &store, &target_id, &unrelated_id);
}

#[test]
fn set_label_updates_field_and_shared_modified_timestamp() {
    let (mut store, target_id, unrelated_id) = store_with_backdated_item();
    let before = store.clone();

    assert!(store.set_label("default", &target_id, "updated label"));

    let item = &store.collections["default"].items[&target_id];
    let before_item = &before.collections["default"].items[&target_id];
    assert_eq!(item.label, "updated label");
    assert_eq!(item.secret, before_item.secret);
    assert_eq!(item.content_type, before_item.content_type);
    assert_eq!(item.attributes, before_item.attributes);
    assert_item_mutation_metadata(&before, &store, &target_id, &unrelated_id);
}

#[test]
fn alias_operations() {
    let mut store = SecretStore::new_default();
    let col = store.create_collection("Test", None);

    store.set_alias("myalias", Some(&col));
    assert_eq!(store.get_alias("myalias"), Some(col.as_str()));

    store.set_alias("myalias", None);
    assert_eq!(store.get_alias("myalias"), None);
}

#[test]
fn list_items() {
    let mut store = SecretStore::new_default();
    store
        .create_item("default", "A", &HashMap::new(), b"a", "text/plain", false)
        .unwrap();
    store
        .create_item("default", "B", &HashMap::new(), b"b", "text/plain", false)
        .unwrap();
    assert_eq!(store.list_items("default").len(), 2);
    assert!(store.list_items("nonexistent").is_empty());
}

#[test]
fn collection_label_operations() {
    let mut store = SecretStore::new_default();
    assert!(store.set_collection_label("default", "Renamed"));
    assert_eq!(store.get_collection_label("default"), Some("Renamed"));
    assert!(!store.set_collection_label("nonexistent", "X"));
}

#[test]
fn operations_on_nonexistent_items_return_false_or_none() {
    let mut store = SecretStore::new_default();
    assert_eq!(store.get_secret("default", "nope"), None);
    assert_eq!(store.get_label("default", "nope"), None);
    assert_eq!(store.get_attributes("default", "nope"), None);
    assert_eq!(store.get_created("default", "nope"), None);
    assert_eq!(store.get_modified("default", "nope"), None);
    assert!(!store.delete_item("default", "nope"));
}

#[test]
fn item_setters_missing_collection_or_item_preserve_exact_state() {
    #[derive(Clone, Copy, Debug)]
    enum Setter {
        Secret,
        Attributes,
        Label,
    }

    #[derive(Clone, Copy, Debug)]
    enum Missing {
        Collection,
        Item,
    }

    for setter in [Setter::Secret, Setter::Attributes, Setter::Label] {
        for missing in [Missing::Collection, Missing::Item] {
            let (mut store, _target_id, _unrelated_id) = store_with_backdated_item();
            let before = store.clone();
            let (collection_id, item_id) = match missing {
                Missing::Collection => ("missing", "item"),
                Missing::Item => ("default", "missing"),
            };

            let changed = match setter {
                Setter::Secret => {
                    store.set_secret(collection_id, item_id, b"synthetic", "text/plain")
                }
                Setter::Attributes => store.set_attributes(collection_id, item_id, HashMap::new()),
                Setter::Label => store.set_label(collection_id, item_id, "label"),
            };
            assert!(!changed, "{setter:?} with missing {missing:?}");
            assert_eq!(store, before, "{setter:?} with missing {missing:?}");
        }
    }
}
