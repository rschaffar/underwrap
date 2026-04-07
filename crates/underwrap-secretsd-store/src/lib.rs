//! In-memory Secret Service state backed by a JSON envelope.
//!
//! This crate provides the data model and serialization for the
//! freedesktop Secret Service state that `underwrap-secretsd` manages.
//! The entire state (collections, items, attributes, labels, timestamps,
//! AND secret values) is stored in a single serializable struct.
//!
//! **Invariant**: The JSON envelope contains secret values. It is NEVER
//! written to a local file. It is only serialized/deserialized over the
//! daemon channel (authenticated socketpair fd) or in tests.
//!
//! The store is intentionally simple: search is O(n) over items in a
//! collection. This is fine because each wrapper has at most a handful
//! of libsecret items.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A complete Secret Service state: collections, items, aliases.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SecretStore {
    /// Map of collection ID → collection.
    pub collections: HashMap<String, Collection>,
    /// Map of alias name → collection ID.
    pub aliases: HashMap<String, String>,
    /// Counter for generating unique IDs.
    #[serde(default)]
    next_id: u64,
}

/// A Secret Service collection containing items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Collection {
    pub label: String,
    pub items: HashMap<String, Item>,
    pub created: u64,
    pub modified: u64,
}

/// A Secret Service item: secret value + metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Item {
    /// The secret value (raw bytes, base64-encoded in JSON for safety).
    #[serde(with = "base64_bytes")]
    pub secret: Vec<u8>,
    /// MIME content type of the secret.
    #[serde(default = "default_content_type")]
    pub content_type: String,
    /// Search attributes (key-value pairs).
    pub attributes: HashMap<String, String>,
    /// Human-readable label.
    pub label: String,
    pub created: u64,
    pub modified: u64,
}

fn default_content_type() -> String {
    "text/plain".to_string()
}

/// Serde helper for base64-encoding Vec<u8> in JSON.
mod base64_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        encoded.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        use base64::Engine;
        let encoded = String::deserialize(d)?;
        base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .map_err(serde::de::Error::custom)
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl SecretStore {
    /// Create a new store with a "default" collection and alias.
    pub fn new_default() -> Self {
        let now = now_secs();
        let mut collections = HashMap::new();
        collections.insert(
            "default".to_string(),
            Collection {
                label: "Default".to_string(),
                items: HashMap::new(),
                created: now,
                modified: now,
            },
        );
        let mut aliases = HashMap::new();
        aliases.insert("default".to_string(), "default".to_string());
        Self {
            collections,
            aliases,
            next_id: 1,
        }
    }

    /// Deserialize from a JSON envelope string.
    pub fn from_json(data: &str) -> Result<Self> {
        serde_json::from_str(data).context("deserializing secret store envelope")
    }

    /// Serialize to a JSON envelope string.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).context("serializing secret store envelope")
    }

    /// Serialize to a pretty-printed JSON envelope string.
    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self).context("serializing secret store envelope")
    }

    fn next_item_id(&mut self) -> String {
        let id = format!("item_{}", self.next_id);
        self.next_id += 1;
        id
    }

    fn next_collection_id(&mut self) -> String {
        let id = format!("col_{}", self.next_id);
        self.next_id += 1;
        id
    }

    // ---------------------------------------------------------------
    // Collections
    // ---------------------------------------------------------------

    /// Create a new collection. Returns the collection ID.
    pub fn create_collection(&mut self, label: Option<&str>, alias: Option<&str>) -> String {
        let now = now_secs();

        // If alias is provided and already points to an existing collection, return it.
        if let Some(alias) = alias
            && let Some(existing_id) = self.aliases.get(alias)
            && self.collections.contains_key(existing_id)
        {
            return existing_id.clone();
        }

        let id = self.next_collection_id();
        let collection = Collection {
            label: label.unwrap_or("Unnamed").to_string(),
            items: HashMap::new(),
            created: now,
            modified: now,
        };
        self.collections.insert(id.clone(), collection);

        if let Some(alias) = alias
            && !alias.is_empty()
        {
            self.aliases.insert(alias.to_string(), id.clone());
        }

        id
    }

    /// Delete a collection and all its items. Returns true if it existed.
    pub fn delete_collection(&mut self, id: &str) -> bool {
        let existed = self.collections.remove(id).is_some();
        if existed {
            // Remove any aliases pointing to this collection.
            self.aliases.retain(|_, v| v != id);
        }
        existed
    }

    /// List all collection IDs.
    pub fn list_collections(&self) -> Vec<&str> {
        self.collections.keys().map(|s| s.as_str()).collect()
    }

    /// Get the collection label.
    pub fn get_collection_label(&self, id: &str) -> Option<&str> {
        self.collections.get(id).map(|c| c.label.as_str())
    }

    /// Set the collection label.
    pub fn set_collection_label(&mut self, id: &str, label: &str) -> bool {
        if let Some(c) = self.collections.get_mut(id) {
            c.label = label.to_string();
            c.modified = now_secs();
            true
        } else {
            false
        }
    }

    /// Get the collection created timestamp.
    pub fn get_collection_created(&self, id: &str) -> Option<u64> {
        self.collections.get(id).map(|c| c.created)
    }

    /// Get the collection modified timestamp.
    pub fn get_collection_modified(&self, id: &str) -> Option<u64> {
        self.collections.get(id).map(|c| c.modified)
    }

    /// Resolve an alias to a collection ID.
    pub fn get_alias(&self, alias: &str) -> Option<&str> {
        self.aliases.get(alias).map(|s| s.as_str())
    }

    /// Set or remove an alias.
    pub fn set_alias(&mut self, alias: &str, collection_id: Option<&str>) {
        match collection_id {
            Some(id) => {
                self.aliases.insert(alias.to_string(), id.to_string());
            }
            None => {
                self.aliases.remove(alias);
            }
        }
    }

    /// List all aliases for a collection.
    pub fn list_aliases_for(&self, collection_id: &str) -> Vec<&str> {
        self.aliases
            .iter()
            .filter(|(_, v)| v.as_str() == collection_id)
            .map(|(k, _)| k.as_str())
            .collect()
    }

    // ---------------------------------------------------------------
    // Items
    // ---------------------------------------------------------------

    /// List all item IDs in a collection.
    pub fn list_items(&self, collection: &str) -> Vec<&str> {
        self.collections
            .get(collection)
            .map(|c| c.items.keys().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    /// Create an item. If `replace` is true and an item with matching
    /// attributes exists, update it instead. Returns `(item_id, replaced)`.
    pub fn create_item(
        &mut self,
        collection: &str,
        label: &str,
        attributes: &HashMap<String, String>,
        secret: &[u8],
        content_type: &str,
        replace: bool,
    ) -> Result<(String, bool)> {
        let now = now_secs();

        // If replace, look for an existing item with matching attributes.
        if replace
            && let Some(col) = self.collections.get_mut(collection)
            && let Some((existing_id, existing)) = col
                .items
                .iter_mut()
                .find(|(_, item)| attrs_match(&item.attributes, attributes))
        {
            existing.secret = secret.to_vec();
            existing.label = label.to_string();
            existing.content_type = content_type.to_string();
            existing.modified = now;
            col.modified = now;
            return Ok((existing_id.clone(), true));
        }

        // Generate ID before borrowing collections.
        let id = self.next_item_id();

        let col = self
            .collections
            .get_mut(collection)
            .context("collection not found")?;

        let item = Item {
            secret: secret.to_vec(),
            content_type: content_type.to_string(),
            attributes: attributes.clone(),
            label: label.to_string(),
            created: now,
            modified: now,
        };
        col.items.insert(id.clone(), item);
        col.modified = now;

        Ok((id, false))
    }

    /// Delete an item. Returns true if it existed.
    pub fn delete_item(&mut self, collection: &str, id: &str) -> bool {
        if let Some(col) = self.collections.get_mut(collection) {
            let existed = col.items.remove(id).is_some();
            if existed {
                col.modified = now_secs();
            }
            existed
        } else {
            false
        }
    }

    /// Search items in a collection by attribute match.
    /// Returns item IDs where all search attributes are present and equal.
    pub fn search(&self, collection: &str, attributes: &HashMap<String, String>) -> Vec<&str> {
        self.collections
            .get(collection)
            .map(|col| {
                col.items
                    .iter()
                    .filter(|(_, item)| attrs_match(&item.attributes, attributes))
                    .map(|(id, _)| id.as_str())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Search all collections. Returns `(collection_id, [item_ids])` pairs.
    pub fn search_all(&self, attributes: &HashMap<String, String>) -> Vec<(&str, Vec<&str>)> {
        self.collections
            .iter()
            .filter_map(|(col_id, col)| {
                let matches: Vec<&str> = col
                    .items
                    .iter()
                    .filter(|(_, item)| attrs_match(&item.attributes, attributes))
                    .map(|(id, _)| id.as_str())
                    .collect();
                if matches.is_empty() {
                    None
                } else {
                    Some((col_id.as_str(), matches))
                }
            })
            .collect()
    }

    /// Get an item's secret value.
    pub fn get_secret(&self, collection: &str, id: &str) -> Option<&[u8]> {
        self.collections
            .get(collection)
            .and_then(|c| c.items.get(id))
            .map(|item| item.secret.as_slice())
    }

    /// Set an item's secret value.
    pub fn set_secret(&mut self, collection: &str, id: &str, value: &[u8]) -> bool {
        if let Some(item) = self
            .collections
            .get_mut(collection)
            .and_then(|c| c.items.get_mut(id))
        {
            item.secret = value.to_vec();
            item.modified = now_secs();
            true
        } else {
            false
        }
    }

    /// Get an item's content type.
    pub fn get_content_type(&self, collection: &str, id: &str) -> Option<&str> {
        self.collections
            .get(collection)
            .and_then(|c| c.items.get(id))
            .map(|item| item.content_type.as_str())
    }

    /// Get an item's attributes.
    pub fn get_attributes(&self, collection: &str, id: &str) -> Option<&HashMap<String, String>> {
        self.collections
            .get(collection)
            .and_then(|c| c.items.get(id))
            .map(|item| &item.attributes)
    }

    /// Set an item's attributes.
    pub fn set_attributes(
        &mut self,
        collection: &str,
        id: &str,
        attrs: HashMap<String, String>,
    ) -> bool {
        if let Some(item) = self
            .collections
            .get_mut(collection)
            .and_then(|c| c.items.get_mut(id))
        {
            item.attributes = attrs;
            item.modified = now_secs();
            true
        } else {
            false
        }
    }

    /// Get an item's label.
    pub fn get_label(&self, collection: &str, id: &str) -> Option<&str> {
        self.collections
            .get(collection)
            .and_then(|c| c.items.get(id))
            .map(|item| item.label.as_str())
    }

    /// Set an item's label.
    pub fn set_label(&mut self, collection: &str, id: &str, label: &str) -> bool {
        if let Some(item) = self
            .collections
            .get_mut(collection)
            .and_then(|c| c.items.get_mut(id))
        {
            item.label = label.to_string();
            item.modified = now_secs();
            true
        } else {
            false
        }
    }

    /// Get an item's created timestamp.
    pub fn get_created(&self, collection: &str, id: &str) -> Option<u64> {
        self.collections
            .get(collection)
            .and_then(|c| c.items.get(id))
            .map(|item| item.created)
    }

    /// Get an item's modified timestamp.
    pub fn get_modified(&self, collection: &str, id: &str) -> Option<u64> {
        self.collections
            .get(collection)
            .and_then(|c| c.items.get(id))
            .map(|item| item.modified)
    }
}

/// Check if `item_attrs` contains all entries from `search_attrs`.
fn attrs_match(
    item_attrs: &HashMap<String, String>,
    search_attrs: &HashMap<String, String>,
) -> bool {
    search_attrs
        .iter()
        .all(|(k, v)| item_attrs.get(k).map(|iv| iv == v).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn create_collection() {
        let mut store = SecretStore::new_default();
        let id = store.create_collection(Some("My Collection"), Some("mine"));
        assert!(store.collections.contains_key(&id));
        assert_eq!(store.get_alias("mine"), Some(id.as_str()));
        assert_eq!(store.get_collection_label(&id), Some("My Collection"));
    }

    #[test]
    fn create_collection_existing_alias_returns_existing() {
        let mut store = SecretStore::new_default();
        let id1 = store.create_collection(Some("First"), Some("myalias"));
        let id2 = store.create_collection(Some("Second"), Some("myalias"));
        assert_eq!(id1, id2, "existing alias should return existing collection");
    }

    #[test]
    fn delete_collection_removes_aliases() {
        let mut store = SecretStore::new_default();
        let id = store.create_collection(Some("Temp"), Some("tempalias"));
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
        assert!(!store.delete_item("default", &id)); // already gone
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

        // Search for service=gh — should find only the gh item.
        let mut search = HashMap::new();
        search.insert("service".to_string(), "gh".to_string());
        let results = store.search("default", &search);
        assert_eq!(results, vec![id_gh.as_str()]);

        // Search for service=gh AND account=user1 — should still match.
        search.insert("account".to_string(), "user1".to_string());
        let results = store.search("default", &search);
        assert_eq!(results, vec![id_gh.as_str()]);

        // Search for service=nonexistent — empty.
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
        let col2 = store.create_collection(Some("Second"), None);

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
        let col = store.create_collection(Some("Test"), None);

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
}
