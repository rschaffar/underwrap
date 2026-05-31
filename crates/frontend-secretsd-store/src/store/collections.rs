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
            next_id: 1,
        }
    }

    pub(crate) fn next_collection_id(&mut self) -> String {
        let id = format!("col_{}", self.next_id);
        self.next_id += 1;
        id
    }

    /// Create a new collection. Returns the collection ID.
    pub fn create_collection(&mut self, label: Option<&str>, alias: Option<&str>) -> String {
        let now = now_secs();

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
