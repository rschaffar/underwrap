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
mod tests;
