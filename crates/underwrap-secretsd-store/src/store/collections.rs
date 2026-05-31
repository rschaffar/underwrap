use std::collections::HashMap;

use super::helpers::now_secs;
use super::{Collection, SecretStore};

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
        }
    }

    /// Create a collection, returning its ID.
    ///
    /// The collection's object-path segment (its id) is its **name**
    /// (the requested Label), not an opaque counter. Secret Service
    /// clients backed by `99designs/keyring` (gog, aws-sso, aws-vault,
    /// …) locate their collection by exact object path rather than by
    /// the `Label` property or an alias, so the path segment has to be
    /// the name they ask for or they never find what they wrote.
    ///
    /// Creation is **idempotent by id**: a client's create-if-missing
    /// becomes a clean no-op on every run after the first, instead of
    /// minting a fresh collection each time. The D-Bus handler validates
    /// the name (non-empty, valid path segment) before calling us, so
    /// there is no anonymous fallback — the name is always usable as the
    /// id.
    pub fn create_collection(&mut self, name: &str, alias: Option<&str>) -> String {
        let now = now_secs();

        if let Some(alias) = alias
            && let Some(existing_id) = self.aliases.get(alias)
            && self.collections.contains_key(existing_id)
        {
            return existing_id.clone();
        }

        if !self.collections.contains_key(name) {
            self.collections.insert(
                name.to_string(),
                Collection {
                    label: name.to_string(),
                    items: HashMap::new(),
                    created: now,
                    modified: now,
                },
            );
        }

        if let Some(alias) = alias
            && !alias.is_empty()
        {
            self.aliases.insert(alias.to_string(), name.to_string());
        }

        name.to_string()
    }

    /// Delete a collection and all its items. Returns true if it existed.
    pub fn delete_collection(&mut self, id: &str) -> bool {
        let existed = self.collections.remove(id).is_some();
        if existed {
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
}
