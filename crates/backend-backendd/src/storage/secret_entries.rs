//! Per-key secret-entry CRUD that translates through the resource-blob
//! layout, plus the structured-id validator shared by every admin
//! secret endpoint.
//!
//! The on-disk shape is one blob per resource at
//! `resources/<resource_id>`. These helpers expand a blob to its
//! flat `<resource>/<slot>/<inner>` entry map for per-key reads /
//! writes / lists, and recompose the blob on each write so the file
//! flip stays atomic.

use anyhow::Result;

use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource::parse_secret_id;
use underwrap_store::{
    StorageBackend, blob_from_resource_entries, expand_blob_to_entries, load_resource_blob,
    save_resource_blob,
};

use crate::admin_api::AdminError;

/// Read a single per-key entry by translating through the resource
/// blob layout. Falls back to raw `store.read` for non-slotted ids
/// (reserved system documents).
pub(crate) fn read_resource_entry(
    store: &dyn StorageBackend,
    secret_id: &str,
) -> Result<Option<SecretBytes>> {
    let Some(parsed) = parse_secret_id(secret_id) else {
        return store.read(secret_id);
    };
    let blob = load_resource_blob(store, parsed.resource_id)?;
    let entries = expand_blob_to_entries(parsed.resource_id, &blob)?;
    Ok(entries.get(secret_id).cloned())
}

/// Write a single per-key entry by load-modify-save against the
/// resource blob.
pub(crate) fn write_resource_entry(
    store: &dyn StorageBackend,
    secret_id: &str,
    material: &[u8],
) -> Result<()> {
    let Some(parsed) = parse_secret_id(secret_id) else {
        return store.write(secret_id, material);
    };
    let blob = load_resource_blob(store, parsed.resource_id)?;
    let mut entries = expand_blob_to_entries(parsed.resource_id, &blob)?;
    entries.insert(secret_id.to_string(), SecretBytes::from(material.to_vec()));
    let new_blob = blob_from_resource_entries(parsed.resource_id, &entries)?;
    save_resource_blob(store, parsed.resource_id, &new_blob)
}

/// Delete a single per-key entry. Returns the existed flag.
pub(crate) fn delete_resource_entry(store: &dyn StorageBackend, secret_id: &str) -> Result<bool> {
    let Some(parsed) = parse_secret_id(secret_id) else {
        return store.delete(secret_id);
    };
    let blob = load_resource_blob(store, parsed.resource_id)?;
    let mut entries = expand_blob_to_entries(parsed.resource_id, &blob)?;
    let existed = entries.remove(secret_id).is_some();
    if existed {
        let new_blob = blob_from_resource_entries(parsed.resource_id, &entries)?;
        save_resource_blob(store, parsed.resource_id, &new_blob)?;
    }
    Ok(existed)
}

/// List per-slot entries across the store, optionally restricted to a
/// single resource. Returns structured `(resource, slot, key)` entries
/// — the flattened in-blob ids never leave the backend.
pub(crate) fn list_secret_entries(
    store: &dyn StorageBackend,
    resource_filter: Option<&str>,
) -> Result<Vec<underwrap_protocol::service::AdminSecretEntry>> {
    use underwrap_protocol::resource::parse_secret_id;
    use underwrap_protocol::service::AdminSecretEntry;

    let resources: Vec<String> = match resource_filter {
        Some(rid) => vec![rid.to_string()],
        None => store
            .list("resources")?
            .into_iter()
            .filter_map(|k| k.strip_prefix("resources/").map(str::to_string))
            .collect(),
    };

    let mut entries: Vec<AdminSecretEntry> = Vec::new();
    for resource_id in &resources {
        let blob = load_resource_blob(store, resource_id)?;
        let blob_entries = expand_blob_to_entries(resource_id, &blob)?;
        for entry_key in blob_entries.into_keys() {
            // `expand_blob_to_entries` produces ids of the canonical
            // `<resource>/<slot>/<inner>` shape; anything else is a
            // bug. Skip silently rather than fail the whole listing.
            let Some(parsed) = parse_secret_id(&entry_key) else {
                continue;
            };
            entries.push(AdminSecretEntry {
                resource_id: parsed.resource_id.to_string(),
                slot: parsed.slot,
                key: parsed.inner.to_string(),
            });
        }
    }
    entries.sort_by(|a, b| {
        a.resource_id
            .cmp(&b.resource_id)
            .then((a.slot as u8).cmp(&(b.slot as u8)))
            .then(a.key.cmp(&b.key))
    });
    Ok(entries)
}

/// Count how many per-slot entries exist under `resource_name`, plus
/// a small sample of ids for the structured "delete refused"
/// response.
pub(crate) fn resource_blob_entries(
    store: &dyn StorageBackend,
    resource_name: &str,
) -> Result<Vec<String>> {
    let blob = load_resource_blob(store, resource_name)?;
    let entries = expand_blob_to_entries(resource_name, &blob)?;
    let mut ids: Vec<String> = entries.into_keys().collect();
    ids.sort();
    Ok(ids)
}

/// Validate the structured `(resource_id, slot, key)` tuple and
/// compose the in-blob entry id. Centralizes the per-slot key-shape
/// rules so every admin secret endpoint enforces them identically.
pub(crate) fn validate_and_compose_secret_id(
    resource_id: &str,
    slot: underwrap_protocol::resource::ResourceSlot,
    key: &str,
) -> std::result::Result<String, AdminError> {
    if resource_id.trim().is_empty() {
        return Err(AdminError::bad_request(
            "invalid_request",
            "resource_id must not be empty",
        ));
    }
    if resource_id.starts_with("__underwrap") {
        return Err(AdminError::bad_request(
            "invalid_request",
            "resource ids starting with '__underwrap' are reserved",
        ));
    }
    if let Err(msg) = underwrap_protocol::resource::validate_secret_key(slot, key) {
        return Err(AdminError::bad_request("invalid_request", msg));
    }
    Ok(underwrap_protocol::resource::compose_secret_id(
        resource_id,
        slot,
        key,
    ))
}
