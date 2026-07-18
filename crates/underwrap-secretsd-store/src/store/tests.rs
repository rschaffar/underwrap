use super::*;
use std::collections::HashMap;

#[test]
fn new_default_has_default_collection() {
    let store = SecretStore::new_default();
    assert!(store.collections.contains_key("default"));
    assert_eq!(store.get_alias("default"), Some("default"));
    assert_eq!(store.get_collection_label("default"), Some("Default"));
}

#[test]
fn json_roundtrip_empty() {
    let store = SecretStore::new_default();
    let json = store.to_json().unwrap();
    let restored = SecretStore::from_json(&json).unwrap();
    assert_eq!(store, restored);
}

#[test]
fn json_roundtrip_with_items() {
    let mut store = SecretStore::new_default();
    let mut attrs = HashMap::new();
    attrs.insert("service".to_string(), "gh".to_string());
    attrs.insert("account".to_string(), "github.com".to_string());

    let (id, replaced) = store
        .create_item(
            "default",
            "GitHub Token",
            &attrs,
            b"ghp_secret123",
            "text/plain",
            false,
        )
        .unwrap();
    assert!(!replaced);

    let json = store.to_json().unwrap();
    let restored = SecretStore::from_json(&json).unwrap();
    assert_eq!(
        restored.get_secret("default", &id),
        Some(b"ghp_secret123".as_slice())
    );
    assert_eq!(restored.get_label("default", &id), Some("GitHub Token"));
    assert_eq!(
        restored
            .get_attributes("default", &id)
            .unwrap()
            .get("service"),
        Some(&"gh".to_string())
    );
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
    assert_eq!(store.get_content_type("default", &id), Some("text/plain"));
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
fn set_and_get_secret() {
    let mut store = SecretStore::new_default();
    let (id, _) = store
        .create_item(
            "default",
            "Test",
            &HashMap::new(),
            b"original",
            "text/plain",
            false,
        )
        .unwrap();
    assert!(store.set_secret("default", &id, b"updated"));
    assert_eq!(
        store.get_secret("default", &id),
        Some(b"updated".as_slice())
    );
}

#[test]
fn set_and_get_attributes() {
    let mut store = SecretStore::new_default();
    let (id, _) = store
        .create_item(
            "default",
            "Test",
            &HashMap::new(),
            b"s",
            "text/plain",
            false,
        )
        .unwrap();
    let mut new_attrs = HashMap::new();
    new_attrs.insert("new_key".to_string(), "new_val".to_string());
    assert!(store.set_attributes("default", &id, new_attrs.clone()));
    assert_eq!(store.get_attributes("default", &id), Some(&new_attrs));
}

#[test]
fn set_and_get_label() {
    let mut store = SecretStore::new_default();
    let (id, _) = store
        .create_item(
            "default",
            "Original",
            &HashMap::new(),
            b"s",
            "text/plain",
            false,
        )
        .unwrap();
    assert!(store.set_label("default", &id, "Renamed"));
    assert_eq!(store.get_label("default", &id), Some("Renamed"));
}

#[test]
fn alias_operations() {
    let mut store = SecretStore::new_default();
    let col = store.create_collection("Test", None);

    store.set_alias("myalias", Some(&col));
    assert_eq!(store.get_alias("myalias"), Some(col.as_str()));

    let aliases = store.list_aliases_for(&col);
    assert!(aliases.contains(&"myalias"));

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
fn binary_secret_roundtrip() {
    let mut store = SecretStore::new_default();
    let binary_secret: Vec<u8> = (0..=255).collect();
    let (id, _) = store
        .create_item(
            "default",
            "Binary",
            &HashMap::new(),
            &binary_secret,
            "application/octet-stream",
            false,
        )
        .unwrap();

    let json = store.to_json().unwrap();
    let restored = SecretStore::from_json(&json).unwrap();
    assert_eq!(
        restored.get_secret("default", &id).unwrap(),
        binary_secret.as_slice()
    );
}

#[test]
fn operations_on_nonexistent_items_return_false_or_none() {
    let mut store = SecretStore::new_default();
    assert_eq!(store.get_secret("default", "nope"), None);
    assert_eq!(store.get_label("default", "nope"), None);
    assert_eq!(store.get_attributes("default", "nope"), None);
    assert_eq!(store.get_created("default", "nope"), None);
    assert_eq!(store.get_modified("default", "nope"), None);
    assert!(!store.set_secret("default", "nope", b"x"));
    assert!(!store.set_label("default", "nope", "x"));
    assert!(!store.set_attributes("default", "nope", HashMap::new()));
    assert!(!store.delete_item("default", "nope"));
}
