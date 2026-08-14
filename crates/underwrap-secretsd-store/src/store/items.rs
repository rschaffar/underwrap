use std::collections::HashMap;

use anyhow::{Context, Result};

use super::helpers::{attrs_match, now_secs};
use super::{Item, SecretStore};

/// Generate a fresh, collision-proof item id.
///
/// Item ids carry no meaning and are only ever resolved by attribute
/// search (`Collection.SearchItems`), never by path or sort order, so a
/// random id is safe. It must remain a valid D-Bus object-path segment
/// (`[A-Za-z0-9_]`), so we hex-encode 16 random bytes (32 chars) — a
/// hyphenated UUID would be an illegal segment. Random ids avoid collisions
/// with items loaded from persisted state, preventing a create from silently
/// overwriting an existing item.
///
/// Fails loudly if the OS RNG is unavailable rather than falling back to
/// a predictable id.
fn random_item_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|e| anyhow::anyhow!("obtaining random bytes for a Secret Service item id: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

impl SecretStore {
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

        let id = random_item_id()?;
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

    pub fn get_secret(&self, collection: &str, id: &str) -> Option<&[u8]> {
        self.collections
            .get(collection)
            .and_then(|c| c.items.get(id))
            .map(|item| item.secret.as_slice())
    }

    pub fn get_attributes(&self, collection: &str, id: &str) -> Option<&HashMap<String, String>> {
        self.collections
            .get(collection)
            .and_then(|c| c.items.get(id))
            .map(|item| &item.attributes)
    }

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

    pub fn get_label(&self, collection: &str, id: &str) -> Option<&str> {
        self.collections
            .get(collection)
            .and_then(|c| c.items.get(id))
            .map(|item| item.label.as_str())
    }

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

    pub fn get_created(&self, collection: &str, id: &str) -> Option<u64> {
        self.collections
            .get(collection)
            .and_then(|c| c.items.get(id))
            .map(|item| item.created)
    }

    pub fn get_modified(&self, collection: &str, id: &str) -> Option<u64> {
        self.collections
            .get(collection)
            .and_then(|c| c.items.get(id))
            .map(|item| item.modified)
    }
}
