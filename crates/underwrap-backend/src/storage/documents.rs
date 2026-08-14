//! Typed load/save helpers for the per-store control-plane
//! documents: the policy document
//! (`POLICY_DOCUMENT_SECRET_ID`), the resources document
//! (`RESOURCES_DOCUMENT_SECRET_ID`), and the pairs document
//! (`PAIRS_DOCUMENT_SECRET_ID`).
//!
//! Each pair of `load_*` / `save_*` functions hides the `serde_json`
//! round-trip so handlers can think in domain terms (`PolicyDocument`,
//! `ResourcesDocument`, `PairsDocument`).

use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;

use underwrap_backend_store::{StorageBackend, load_resource_blob};
use underwrap_protocol::entities::{
    PAIRS_DOCUMENT_SECRET_ID, PairsDocument, RESOURCES_DOCUMENT_SECRET_ID, ResourcesDocument,
};
use underwrap_protocol::policy::{POLICY_DOCUMENT_SECRET_ID, PolicyDocument};
use underwrap_protocol::resource_blob::ResourceContentReference;

/// Sample size for the "delete refused: dependents exist" structured
/// responses across resource / pair / wrapper-permission admin
/// handlers.
pub(crate) const ENTITY_DEPENDENT_SAMPLE_LIMIT: usize = 5;

/// One of the three per-store control-plane documents (policy /
/// resources / pairs). The trait pins the on-disk secret id and a
/// short human-readable name used in error context, so the generic
/// load/save helpers don't have to be parameterised over either.
pub(crate) trait Document: Default + Serialize + DeserializeOwned {
    const SECRET_ID: &'static str;
    const NAME: &'static str;
}

impl Document for PolicyDocument {
    const SECRET_ID: &'static str = POLICY_DOCUMENT_SECRET_ID;
    const NAME: &'static str = "policy";
}

impl Document for ResourcesDocument {
    const SECRET_ID: &'static str = RESOURCES_DOCUMENT_SECRET_ID;
    const NAME: &'static str = "resources";
}

impl Document for PairsDocument {
    const SECRET_ID: &'static str = PAIRS_DOCUMENT_SECRET_ID;
    const NAME: &'static str = "pairs";
}

/// Load a control-plane document by its [`Document::SECRET_ID`].
/// Treats a missing blob as the document's default — that is the
/// expected state on a fresh store.
pub(crate) fn load_document<T: Document>(store: &dyn StorageBackend) -> Result<T> {
    match store.read(T::SECRET_ID)? {
        Some(material) => serde_json::from_slice(material.as_ref())
            .with_context(|| format!("parsing backend {} document", T::NAME)),
        None => Ok(T::default()),
    }
}

/// Persist a control-plane document under its [`Document::SECRET_ID`].
pub(crate) fn save_document<T: Document>(store: &dyn StorageBackend, doc: &T) -> Result<()> {
    store.write(T::SECRET_ID, serde_json::to_vec(doc)?.as_slice())
}

// Thin per-document aliases. Each forwards to the generic helper above
// so call sites stay readable (`load_policy_document(store)` reads
// better than `load_document::<PolicyDocument>(store)` at the seven
// admin/runtime handlers that consume these documents).

pub(crate) fn load_policy_document(store: &dyn StorageBackend) -> Result<PolicyDocument> {
    load_document(store)
}
pub(crate) fn save_policy_document(store: &dyn StorageBackend, doc: &PolicyDocument) -> Result<()> {
    save_document(store, doc)
}
pub(crate) fn load_resources_document(store: &dyn StorageBackend) -> Result<ResourcesDocument> {
    load_document(store)
}
pub(crate) fn save_resources_document(
    store: &dyn StorageBackend,
    doc: &ResourcesDocument,
) -> Result<()> {
    save_document(store, doc)
}
pub(crate) fn load_pairs_document(store: &dyn StorageBackend) -> Result<PairsDocument> {
    load_document(store)
}
pub(crate) fn save_pairs_document(store: &dyn StorageBackend, doc: &PairsDocument) -> Result<()> {
    save_document(store, doc)
}

/// Count every typed content entry in one resource blob and return a capped,
/// deterministic identifier-only sample. A genuinely missing active key is
/// represented by `load_resource_blob` as an empty blob; every read/decode/
/// size/version failure is returned to the caller.
pub(crate) fn resource_content_dependents(
    store: &dyn StorageBackend,
    resource_id: &str,
) -> Result<(u32, Vec<ResourceContentReference>)> {
    let mut references = load_resource_blob(store, resource_id)?.content_references();
    let count = crate::admin_api::util::saturating_u32(references.len());
    references.truncate(ENTITY_DEPENDENT_SAMPLE_LIMIT);
    Ok((count, references))
}

/// Count permissions in the store's policy document that match
/// `predicate`, plus a [`ENTITY_DEPENDENT_SAMPLE_LIMIT`]-sized sample
/// of their `(client_instance, wrapper, resource_id)` keys for the
/// structured "delete refused" responses on pair and resource
/// delete.
pub(crate) fn policy_permission_dependents(
    store: &dyn StorageBackend,
    predicate: impl Fn(&underwrap_protocol::policy::WrapperPermission) -> bool,
) -> Result<(
    u32,
    Vec<underwrap_protocol::service::AdminWrapperPermissionKey>,
)> {
    use underwrap_protocol::service::AdminWrapperPermissionKey;
    let policy: PolicyDocument = load_document(store)?;
    let dependents: Vec<AdminWrapperPermissionKey> = policy
        .permissions
        .iter()
        .filter(|p| predicate(p))
        .map(|p| AdminWrapperPermissionKey {
            client_instance: p.client_instance.clone(),
            wrapper: p.wrapper.clone(),
            resource_id: p.resource_id.clone(),
        })
        .collect();
    let count = crate::admin_api::util::saturating_u32(dependents.len());
    let mut sample = dependents;
    sample.truncate(ENTITY_DEPENDENT_SAMPLE_LIMIT);
    Ok((count, sample))
}

#[cfg(test)]
mod tests;
