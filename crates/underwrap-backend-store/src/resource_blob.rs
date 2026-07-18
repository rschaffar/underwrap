//! Helpers for reading and writing canonical resource blobs and for projecting
//! their `env` and `files` sections into the flat entry shape used by the admin
//! secret endpoints.
//!
//! The storage backend is opaque per-key: keys are strings, values are
//! bytes. The blob layout sits one layer above that — every resource
//! is one storage key (`resources/<resource_id>`) whose value is the
//! JCS-canonical bytes of a `ResourceBlob`.
//!
//! The flat entry projection covers the slots that are naturally key/value:
//! `env` (name → value) and `files` (path → bytes). The
//! `libsecret` section is a structured tree (Service → Collections →
//! Items + an alias table) addressed through its own admin endpoints
//! (`/v1/admin/stores/{sid}/resources/{rid}/libsecret/*`); it never
//! travels through this bridge.
//!
//! See [`underwrap_protocol::resource_blob`] for the schema and
//! canonicalization rules.

use std::collections::HashMap;

use anyhow::{Context, Result};
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource::resource_blob_key;
use underwrap_protocol::resource_blob::ResourceBlob;

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

/// Expand a single resource's blob into the per-key flat-KV entries
/// the admin `env` / `files` endpoints address. Libsecret content is
/// **not** projected here — admin operations on the libsecret tree go
/// through the structured `/libsecret/*` endpoints instead, which
/// read and write the structured section directly.
///
/// Output keys are of the form:
///
/// - `<resource_id>/env/<VAR>`
/// - `<resource_id>/files/<path>`
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
    Ok(out)
}

#[cfg(test)]
mod tests;
