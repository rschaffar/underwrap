//! Backend-side helpers over the per-store `dyn StorageBackend`.
//!
//! Submodules:
//!
//! - [`documents`] — typed load/save helpers for the per-store
//!   policy, resources, and pairs documents.
//! - [`secret_entries`] — translate per-key admin reads/writes
//!   through the resource-blob layout, plus the structured-id
//!   validator shared by every admin secret endpoint.

pub(crate) mod documents;
pub(crate) mod secret_entries;
