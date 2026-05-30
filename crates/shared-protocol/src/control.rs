//! The reserved control-plane namespace inside a store backend.
//!
//! A store backend is a flat key→bytes map. User secret material lives
//! under `resources/<id>` — one canonical blob per resource, where the
//! id is a server-generated `res_*` token (see the backend's `ids`
//! module) and therefore can never collide with the names below.
//!
//! Everything in the store that is *not* user secret material — the
//! policy / resources / pairs control-plane documents and the
//! encrypted-store unlock canary — lives under the single reserved
//! [`CONTROL_PREFIX`]. There is exactly one reserved namespace and one
//! rule: a key is user secret material iff it is **not**
//! [`is_control_key`]. This replaces the earlier split between a
//! `__underwrap/` document prefix and a separate `__underwrap__/`
//! canary id, each of which used its own ad-hoc filtering.

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
