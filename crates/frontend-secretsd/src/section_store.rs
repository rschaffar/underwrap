//! Conversions between the protocol-level `LibsecretSection` (the
//! shape used in the resource blob and on the ns↔secretsd pipes) and
//! the runtime `SecretStore` (the shape served over D-Bus).
//!
//! The shapes are deliberately similar but differ on container types
//! (`BTreeMap` in the protocol so canonicalization is deterministic;
//! `HashMap` in the runtime store for cheap lookups) and on a few
//! field encodings.

use std::collections::HashMap;

use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource_blob::{Collection, Item, LibsecretSection};
use underwrap_secretsd_store::{Collection as StoreCollection, Item as StoreItem, SecretStore};

/// Build a fresh `SecretStore` from the libsecret section the
/// daemon handed us at `Checkout`. The store is the in-process
/// source of truth for the namespace lifetime.
pub fn store_from_section(section: &LibsecretSection) -> SecretStore {
    let mut store = SecretStore::new_default();
    // `new_default` seeds a "default" collection + alias. The
    // section is the authoritative state, so replace both.
    store.collections.clear();
    store.aliases.clear();

    for (col_id, col) in &section.collections {
        let mut items = HashMap::with_capacity(col.items.len());
        for (item_id, item) in &col.items {
            items.insert(
                item_id.clone(),
                StoreItem {
                    secret: Vec::<u8>::from(item.secret.clone()),
                    content_type: item.content_type.clone(),
                    attributes: item
                        .attributes
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                    label: item.label.clone(),
                    created: item.created,
                    modified: item.modified,
                },
            );
        }
        store.collections.insert(
            col_id.clone(),
            StoreCollection {
                label: col.label.clone(),
                items,
                created: col.created,
                modified: col.modified,
            },
        );
    }
    for (alias, col_id) in &section.aliases {
        store.aliases.insert(alias.clone(), col_id.clone());
    }
    // Ensure a "default" collection exists, per Secret Service spec.
    if !store.collections.contains_key("default") {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        store.collections.insert(
            "default".to_string(),
            StoreCollection {
                label: "Default".to_string(),
                items: HashMap::new(),
                created: now,
                modified: now,
            },
        );
    }
    if !store.aliases.contains_key("default") {
        store
            .aliases
            .insert("default".to_string(), "default".to_string());
    }
    store
}

/// Project the current `SecretStore` back into a `LibsecretSection`
/// for write-back. Used to dump state to the on-tmpfs state file on
/// every mutation, and to compose the final teardown blob.
pub fn section_from_store(store: &SecretStore) -> LibsecretSection {
    let mut section = LibsecretSection::default();
    for (col_id, col) in &store.collections {
        let mut items = std::collections::BTreeMap::new();
        for (item_id, item) in &col.items {
            items.insert(
                item_id.clone(),
                Item {
                    label: item.label.clone(),
                    content_type: item.content_type.clone(),
                    created: item.created,
                    modified: item.modified,
                    attributes: item
                        .attributes
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                    secret: SecretBytes::from(item.secret.clone()),
                },
            );
        }
        section.collections.insert(
            col_id.clone(),
            Collection {
                label: col.label.clone(),
                created: col.created,
                modified: col.modified,
                items,
            },
        );
    }
    for (alias, col_id) in &store.aliases {
        section.aliases.insert(alias.clone(), col_id.clone());
    }
    section
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn sample_section() -> LibsecretSection {
        let mut section = LibsecretSection::default();
        let mut attrs = BTreeMap::new();
        attrs.insert("server".into(), "github.com".into());

        let mut items = BTreeMap::new();
        items.insert(
            "gh-cli".into(),
            Item {
                label: "gh CLI".into(),
                content_type: "text/plain".into(),
                created: 100,
                modified: 200,
                attributes: attrs,
                secret: SecretBytes::from("ghp_secret"),
            },
        );
        section.collections.insert(
            "default".into(),
            Collection {
                label: "Default".into(),
                created: 10,
                modified: 20,
                items,
            },
        );
        section.aliases.insert("default".into(), "default".into());
        section
    }

    #[test]
    fn round_trip_via_store_preserves_data() {
        let section = sample_section();
        let store = store_from_section(&section);
        let back = section_from_store(&store);
        assert_eq!(back, section);
    }

    #[test]
    fn store_from_empty_section_seeds_default_collection() {
        let store = store_from_section(&LibsecretSection::default());
        assert!(store.collections.contains_key("default"));
        assert_eq!(
            store.aliases.get("default").map(String::as_str),
            Some("default")
        );
    }

    #[test]
    fn store_from_section_preserves_secret_bytes() {
        let section = sample_section();
        let store = store_from_section(&section);
        let secret = store
            .collections
            .get("default")
            .and_then(|c| c.items.get("gh-cli"))
            .map(|item| item.secret.as_slice());
        assert_eq!(secret, Some(b"ghp_secret".as_slice()));
    }
}
