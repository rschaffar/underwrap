//! Typed load/save helpers for the per-store control-plane
//! documents: the policy document
//! (`POLICY_DOCUMENT_SECRET_ID`), the resources document
//! (`RESOURCES_DOCUMENT_SECRET_ID`), and the pairs document
//! (`PAIRS_DOCUMENT_SECRET_ID`).
//!
//! Each pair of `load_*` / `save_*` functions hides the schema-version
//! check and the `serde_json` round-trip so handlers can think in
//! domain terms (`PolicyDocument`, `ResourcesDocument`,
//! `PairsDocument`).

use anyhow::{Context, Result};

use underwrap_protocol::policy::{POLICY_DOCUMENT_SECRET_ID, PolicyDocument};
use underwrap_store::StorageBackend;

use crate::storage::secret_entries::resource_blob_entries;

/// Sample size for the "delete refused: dependents exist" structured
/// responses across resource / pair / wrapper-permission admin
/// handlers.
pub(crate) const ENTITY_DEPENDENT_SAMPLE_LIMIT: usize = 5;

const RESERVED_PREFIX: &str = "__underwrap/";

pub(crate) fn load_policy_document(store: &dyn StorageBackend) -> Result<PolicyDocument> {
    match store.read(POLICY_DOCUMENT_SECRET_ID)? {
        Some(material) => {
            serde_json::from_slice(material.as_ref()).context("parsing backend policy document")
        }
        None => Ok(PolicyDocument::default()),
    }
}

pub(crate) fn load_resources_document(
    store: &dyn StorageBackend,
) -> Result<underwrap_protocol::entities::ResourcesDocument> {
    use underwrap_protocol::entities::{
        ENTITY_SCHEMA_VERSION, RESOURCES_DOCUMENT_SECRET_ID, ResourcesDocument,
    };
    match store.read(RESOURCES_DOCUMENT_SECRET_ID)? {
        Some(material) => {
            let doc: ResourcesDocument = serde_json::from_slice(material.as_ref())
                .context("parsing backend resources document")?;
            anyhow::ensure!(
                doc.version == ENTITY_SCHEMA_VERSION,
                "unsupported resources document version {}",
                doc.version,
            );
            Ok(doc)
        }
        None => Ok(ResourcesDocument::default()),
    }
}

pub(crate) fn save_resources_document(
    store: &dyn StorageBackend,
    doc: &underwrap_protocol::entities::ResourcesDocument,
) -> Result<()> {
    use underwrap_protocol::entities::RESOURCES_DOCUMENT_SECRET_ID;
    store.write(
        RESOURCES_DOCUMENT_SECRET_ID,
        serde_json::to_vec(doc)?.as_slice(),
    )
}

pub(crate) fn load_pairs_document(
    store: &dyn StorageBackend,
) -> Result<underwrap_protocol::entities::PairsDocument> {
    use underwrap_protocol::entities::{
        ENTITY_SCHEMA_VERSION, PAIRS_DOCUMENT_SECRET_ID, PairsDocument,
    };
    match store.read(PAIRS_DOCUMENT_SECRET_ID)? {
        Some(material) => {
            let doc: PairsDocument = serde_json::from_slice(material.as_ref())
                .context("parsing backend pairs document")?;
            anyhow::ensure!(
                doc.version == ENTITY_SCHEMA_VERSION,
                "unsupported pairs document version {}",
                doc.version,
            );
            Ok(doc)
        }
        None => Ok(PairsDocument::default()),
    }
}

pub(crate) fn save_pairs_document(
    store: &dyn StorageBackend,
    doc: &underwrap_protocol::entities::PairsDocument,
) -> Result<()> {
    use underwrap_protocol::entities::PAIRS_DOCUMENT_SECRET_ID;
    store.write(
        PAIRS_DOCUMENT_SECRET_ID,
        serde_json::to_vec(doc)?.as_slice(),
    )
}

pub(crate) fn save_policy_document(store: &dyn StorageBackend, doc: &PolicyDocument) -> Result<()> {
    store.write(
        POLICY_DOCUMENT_SECRET_ID,
        serde_json::to_vec(doc)?.as_slice(),
    )
}

pub(crate) fn resource_secret_dependents(
    store: &dyn StorageBackend,
    resource_name: &str,
) -> Result<(u32, Vec<String>)> {
    let mut ids = resource_blob_entries(store, resource_name)?;
    ids.retain(|id| !id.starts_with(RESERVED_PREFIX));
    ids.sort();
    ids.dedup();
    let count = u32::try_from(ids.len()).unwrap_or(u32::MAX);
    ids.truncate(ENTITY_DEPENDENT_SAMPLE_LIMIT);
    Ok((count, ids))
}

#[cfg(test)]
mod tests;
