//! The reserved control-plane namespace inside a store backend.
//!
//! A store backend is a flat key→bytes map. User secret material lives
//! under `resources/<id>` — one canonical blob per resource, where the
//! id is a server-generated `res_*` token (see the backend's `ids`
//! module) and therefore can never collide with the names below.
//!
//! Everything in the store that is *not* user secret material — policy,
//! resource and pair documents, plus the encrypted-store unlock canary — lives
//! under [`CONTROL_PREFIX`]. A key is user secret material exactly when
//! [`is_control_key`] is false.

/// Reserved top-level namespace for non-secret, store-internal keys.
/// The control-plane document ids and the encrypted-store canary id
/// are all anchored under this prefix.
pub const CONTROL_PREFIX: &str = "control/";

/// True if `key` addresses a reserved control-plane entry (a
/// control-plane document or the unlock canary) rather than user
/// secret material. This is the single authority for "is this an
/// internal key?" across the backend's listing, empty-store, and
/// dependent-counting paths.
pub fn is_control_key(key: &str) -> bool {
    key.starts_with(CONTROL_PREFIX)
}

#[cfg(test)]
mod tests;
