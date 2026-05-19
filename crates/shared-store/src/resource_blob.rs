//! Helpers for reading and writing the canonical per-resource blob,
//! plus a back-compat bridge to/from the legacy in-memory per-key
//! entries shape that secretsd / directory channels still consume.
//!
//! The storage backend is opaque per-key: keys are strings, values are
//! bytes. The blob layout sits one layer above that — every resource
//! is one storage key (`resources/<resource_id>`) whose value is the
//! JCS-canonical bytes of a `ResourceBlob`.
//!
//! The bridge functions exist because the wrapper-side IPC still
//! addresses entries individually by `<resource_id>/<slot>/<inner>`
//! ids; story 410 will rewire wrappers to operate on the structured
//! `ResourceBlob` directly and these bridge functions will go away.
//!
//! See [`underwrap_protocol::resource_blob`] for the schema and
//! canonicalization rules.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource::resource_blob_key;
use underwrap_protocol::resource_blob::{Collection, Item, LibsecretSection, ResourceBlob};

use crate::StorageBackend;

/// Load the resource blob from storage, or return an empty blob if
/// the resource has never been written. A missing blob is distinct
/// from an empty one only in the storage layer; callers treat them
/// the same.
pub fn load_resource_blob(store: &dyn StorageBackend, resource_id: &str) -> Result<ResourceBlob> {
    let key = resource_blob_key(resource_id);
    match store
        .read(&key)
        .with_context(|| format!("reading resource blob '{key}'"))?
    {
        Some(material) => ResourceBlob::from_canonical_bytes(material.as_ref())
            .with_context(|| format!("decoding resource blob '{key}'")),
        None => Ok(ResourceBlob::empty()),
    }
}

/// Save the resource blob to storage by canonicalizing first and
/// writing the bytes atomically (the backend's write contract is
/// atomic where possible).
pub fn save_resource_blob(
    store: &dyn StorageBackend,
    resource_id: &str,
    blob: &ResourceBlob,
) -> Result<()> {
    let key = resource_blob_key(resource_id);
    let bytes = blob.to_canonical_bytes()?;
    store
        .write(&key, &bytes)
        .with_context(|| format!("writing resource blob '{key}'"))
}

/// Delete the resource blob entirely. Returns `Ok(true)` if the blob
/// existed, `Ok(false)` if it did not.
pub fn delete_resource_blob(store: &dyn StorageBackend, resource_id: &str) -> Result<bool> {
    let key = resource_blob_key(resource_id);
    store
        .delete(&key)
        .with_context(|| format!("deleting resource blob '{key}'"))
}

/// Expand a single resource's blob into the per-key in-memory entries
/// the legacy wrapper-side channels consume.
///
/// Output keys are of the form:
///
/// - `<resource_id>/env/<VAR>`
/// - `<resource_id>/files/<path>`
/// - `<resource_id>/libsecret/_aliases`
/// - `<resource_id>/libsecret/<col>/_meta`
/// - `<resource_id>/libsecret/<col>/<item>.meta`
/// - `<resource_id>/libsecret/<col>/<item>.secret`
pub fn expand_blob_to_entries(
    resource_id: &str,
    blob: &ResourceBlob,
) -> Result<HashMap<String, SecretBytes>> {
    let mut out = HashMap::new();
    for (name, value) in &blob.env {
        out.insert(format!("{resource_id}/env/{name}"), value.clone());
    }
    for (path, value) in &blob.files {
        out.insert(format!("{resource_id}/files/{path}"), value.clone());
    }
    for (inner, value) in libsecret_section_to_kv(&blob.libsecret)? {
        out.insert(format!("{resource_id}/libsecret/{inner}"), value);
    }
    Ok(out)
}

/// Inverse of [`expand_blob_to_entries`]: rebuild a structured
/// `ResourceBlob` from the per-key entries that flowed through a
/// wrapper run. Only entries matching `<resource_id>/...` are
/// considered; anything else is silently ignored.
///
/// Used at end-of-channel to persist the run's mutations.
pub fn blob_from_resource_entries(
    resource_id: &str,
    entries: &HashMap<String, SecretBytes>,
) -> Result<ResourceBlob> {
    let env_prefix = format!("{resource_id}/env/");
    let files_prefix = format!("{resource_id}/files/");
    let libsecret_prefix = format!("{resource_id}/libsecret/");

    let mut blob = ResourceBlob::empty();
    let mut libsecret_kv: BTreeMap<String, SecretBytes> = BTreeMap::new();

    for (full_id, value) in entries {
        if let Some(name) = full_id.strip_prefix(&env_prefix) {
            blob.env.insert(name.to_string(), value.clone());
        } else if let Some(path) = full_id.strip_prefix(&files_prefix) {
            blob.files.insert(path.to_string(), value.clone());
        } else if let Some(inner) = full_id.strip_prefix(&libsecret_prefix) {
            libsecret_kv.insert(inner.to_string(), value.clone());
        }
    }

    blob.libsecret = libsecret_section_from_kv(&libsecret_kv)?;
    Ok(blob)
}

/// Convert a `LibsecretSection` into the flat key/value shape secretsd
/// uses in its in-memory bundle. The inner JSON shape for `_meta` and
/// `.meta` matches what secretsd serializes today, so a `Read` after a
/// `Write` round-trips without semantic change.
fn libsecret_section_to_kv(section: &LibsecretSection) -> Result<BTreeMap<String, SecretBytes>> {
    let mut out = BTreeMap::new();

    if !section.aliases.is_empty() || !section.collections.is_empty() {
        let aliases_json =
            serde_json::to_vec(&section.aliases).context("encoding libsecret _aliases")?;
        out.insert("_aliases".to_string(), SecretBytes::from(aliases_json));
    }

    for (col_id, col) in &section.collections {
        let meta = WireCollectionMeta {
            label: &col.label,
            created: col.created,
            modified: col.modified,
        };
        let meta_json = serde_json::to_vec(&meta).context("encoding libsecret collection meta")?;
        out.insert(format!("{col_id}/_meta"), SecretBytes::from(meta_json));

        for (item_id, item) in &col.items {
            let item_meta = WireItemMeta {
                label: &item.label,
                content_type: &item.content_type,
                attributes: &item.attributes,
                created: item.created,
                modified: item.modified,
            };
            let item_meta_json =
                serde_json::to_vec(&item_meta).context("encoding libsecret item meta")?;
            out.insert(
                format!("{col_id}/{item_id}.meta"),
                SecretBytes::from(item_meta_json),
            );
            out.insert(format!("{col_id}/{item_id}.secret"), item.secret.clone());
        }
    }

    Ok(out)
}

/// Inverse of [`libsecret_section_to_kv`]. Parses the flat KV map that
/// flowed through a wrapper run and produces a `LibsecretSection`.
/// Orphan items (`.secret` without a `.meta`, or either without a
/// containing `_meta`) are dropped silently — the same scrub-on-load
/// behavior secretsd already performs.
fn libsecret_section_from_kv(kv: &BTreeMap<String, SecretBytes>) -> Result<LibsecretSection> {
    let mut aliases: BTreeMap<String, String> = BTreeMap::new();
    let mut collections: BTreeMap<String, Collection> = BTreeMap::new();

    if let Some(value) = kv.get("_aliases") {
        aliases = serde_json::from_slice(value.as_ref()).context("parsing libsecret _aliases")?;
    }

    for (key, value) in kv {
        let Some(col_id) = key.strip_suffix("/_meta") else {
            continue;
        };
        let meta: WireCollectionMetaOwned = serde_json::from_slice(value.as_ref())
            .with_context(|| format!("parsing libsecret collection meta '{key}'"))?;
        collections.insert(
            col_id.to_string(),
            Collection {
                label: meta.label,
                created: meta.created,
                modified: meta.modified,
                items: BTreeMap::new(),
            },
        );
    }

    let mut item_metas: BTreeMap<(String, String), WireItemMetaOwned> = BTreeMap::new();
    let mut item_secrets: BTreeMap<(String, String), SecretBytes> = BTreeMap::new();

    for (key, value) in kv {
        if key == "_aliases" || key.ends_with("/_meta") {
            continue;
        }
        if let Some(stem) = key.strip_suffix(".meta")
            && let Some((col, item)) = stem.split_once('/')
        {
            let meta: WireItemMetaOwned = serde_json::from_slice(value.as_ref())
                .with_context(|| format!("parsing libsecret item meta '{key}'"))?;
            item_metas.insert((col.to_string(), item.to_string()), meta);
        } else if let Some(stem) = key.strip_suffix(".secret")
            && let Some((col, item)) = stem.split_once('/')
        {
            item_secrets.insert((col.to_string(), item.to_string()), value.clone());
        }
    }

    for ((col_id, item_id), meta) in item_metas {
        let Some(secret) = item_secrets.remove(&(col_id.clone(), item_id.clone())) else {
            continue;
        };
        let Some(col) = collections.get_mut(&col_id) else {
            continue;
        };
        col.items.insert(
            item_id,
            Item {
                label: meta.label,
                content_type: meta.content_type,
                attributes: meta.attributes,
                created: meta.created,
                modified: meta.modified,
                secret,
            },
        );
    }

    Ok(LibsecretSection {
        aliases,
        collections,
    })
}

#[derive(Serialize)]
struct WireCollectionMeta<'a> {
    label: &'a str,
    created: u64,
    modified: u64,
}

#[derive(Deserialize)]
struct WireCollectionMetaOwned {
    label: String,
    created: u64,
    modified: u64,
}

#[derive(Serialize)]
struct WireItemMeta<'a> {
    label: &'a str,
    content_type: &'a str,
    attributes: &'a BTreeMap<String, String>,
    created: u64,
    modified: u64,
}

#[derive(Deserialize)]
struct WireItemMetaOwned {
    label: String,
    content_type: String,
    #[serde(default)]
    attributes: BTreeMap<String, String>,
    created: u64,
    modified: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use underwrap_protocol::message::SecretBytes;

    /// Minimal in-memory backend, just for these helpers.
    struct MemStore {
        data: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MemStore {
        fn new() -> Self {
            Self {
                data: Mutex::new(HashMap::new()),
            }
        }
    }

    impl StorageBackend for MemStore {
        fn read(&self, secret_id: &str) -> Result<Option<SecretBytes>> {
            Ok(self
                .data
                .lock()
                .unwrap()
                .get(secret_id)
                .cloned()
                .map(SecretBytes::from))
        }
        fn write(&self, secret_id: &str, material: &[u8]) -> Result<()> {
            self.data
                .lock()
                .unwrap()
                .insert(secret_id.into(), material.to_vec());
            Ok(())
        }
        fn list(&self, prefix: &str) -> Result<Vec<String>> {
            Ok(self
                .data
                .lock()
                .unwrap()
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect())
        }
        fn delete(&self, secret_id: &str) -> Result<bool> {
            Ok(self.data.lock().unwrap().remove(secret_id).is_some())
        }
    }

    #[test]
    fn load_returns_empty_blob_when_missing() {
        let store = MemStore::new();
        let blob = load_resource_blob(&store, "github/token").unwrap();
        assert_eq!(blob, ResourceBlob::empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let store = MemStore::new();
        let mut blob = ResourceBlob::empty();
        blob.env
            .insert("GH_TOKEN".into(), SecretBytes::from("ghp_test"));
        save_resource_blob(&store, "github/token", &blob).unwrap();
        let loaded = load_resource_blob(&store, "github/token").unwrap();
        assert_eq!(loaded, blob);
    }

    #[test]
    fn save_uses_resources_key_shape() {
        let store = MemStore::new();
        save_resource_blob(&store, "github/token", &ResourceBlob::empty()).unwrap();
        assert!(
            store
                .data
                .lock()
                .unwrap()
                .contains_key("resources/github/token"),
            "expected key 'resources/github/token', got {:?}",
            store.data.lock().unwrap().keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn delete_returns_existed_flag() {
        let store = MemStore::new();
        save_resource_blob(&store, "r", &ResourceBlob::empty()).unwrap();
        assert!(delete_resource_blob(&store, "r").unwrap());
        assert!(!delete_resource_blob(&store, "r").unwrap());
    }

    fn populated_blob() -> ResourceBlob {
        let mut blob = ResourceBlob::empty();
        blob.env
            .insert("GH_TOKEN".into(), SecretBytes::from("ghp_test"));
        blob.files
            .insert("config/app.toml".into(), SecretBytes::from("x=1\n"));

        let mut items = BTreeMap::new();
        let mut attrs = BTreeMap::new();
        attrs.insert("server".into(), "github.com".into());
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

        let mut collections = BTreeMap::new();
        collections.insert(
            "default".into(),
            Collection {
                label: "Default".into(),
                created: 10,
                modified: 20,
                items,
            },
        );

        let mut aliases = BTreeMap::new();
        aliases.insert("default".into(), "default".into());

        blob.libsecret = LibsecretSection {
            aliases,
            collections,
        };
        blob
    }

    #[test]
    fn expand_and_roundtrip_blob_to_entries() {
        let blob = populated_blob();
        let entries = expand_blob_to_entries("github/token", &blob).unwrap();

        assert_eq!(
            entries.get("github/token/env/GH_TOKEN").unwrap(),
            &SecretBytes::from("ghp_test")
        );
        assert_eq!(
            entries.get("github/token/files/config/app.toml").unwrap(),
            &SecretBytes::from("x=1\n")
        );
        assert!(entries.contains_key("github/token/libsecret/_aliases"));
        assert!(entries.contains_key("github/token/libsecret/default/_meta"));
        assert!(entries.contains_key("github/token/libsecret/default/gh-cli.meta"));
        assert_eq!(
            entries
                .get("github/token/libsecret/default/gh-cli.secret")
                .unwrap(),
            &SecretBytes::from("ghp_secret")
        );

        let rebuilt = blob_from_resource_entries("github/token", &entries).unwrap();
        assert_eq!(rebuilt, blob);
    }

    #[test]
    fn blob_from_resource_entries_ignores_other_resources() {
        let mut entries = HashMap::new();
        entries.insert("a/env/X".to_string(), SecretBytes::from("a"));
        entries.insert("b/env/Y".to_string(), SecretBytes::from("b"));

        let blob = blob_from_resource_entries("a", &entries).unwrap();
        assert_eq!(blob.env.len(), 1);
        assert_eq!(blob.env.get("X").unwrap(), &SecretBytes::from("a"));
    }

    #[test]
    fn blob_from_resource_entries_drops_orphan_libsecret_items() {
        // A .secret without a matching .meta should be dropped silently.
        let mut entries = HashMap::new();
        // _aliases + collection meta + orphan secret
        entries.insert(
            "r/libsecret/_aliases".to_string(),
            SecretBytes::from(serde_json::to_vec(&BTreeMap::<String, String>::new()).unwrap()),
        );
        entries.insert(
            "r/libsecret/default/_meta".to_string(),
            SecretBytes::from(
                serde_json::to_vec(&serde_json::json!({
                    "label": "Default",
                    "created": 0u64,
                    "modified": 0u64,
                }))
                .unwrap(),
            ),
        );
        entries.insert(
            "r/libsecret/default/orphan.secret".to_string(),
            SecretBytes::from("x"),
        );

        let blob = blob_from_resource_entries("r", &entries).unwrap();
        let col = blob.libsecret.collections.get("default").unwrap();
        assert!(col.items.is_empty());
    }
}
