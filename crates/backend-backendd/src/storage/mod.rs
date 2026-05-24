//! Backend-side helpers over the per-store `dyn StorageBackend`.
//!
//! Submodules:
//!
//! - [`documents`] — typed load/save helpers for the per-store
//!   policy, resources, and pairs documents.
//! - [`json_dir_store`] — generic `<dir>/<id>.json` registry used by
//!   the users, stores, and runtime-clients on-disk collections.
//! - [`secret_entries`] — translate per-key admin reads/writes
//!   through the resource-blob layout, plus the structured-id
//!   validator shared by every admin secret endpoint.

pub(crate) mod documents;
pub(crate) mod json_dir_store;
pub(crate) mod secret_entries;
