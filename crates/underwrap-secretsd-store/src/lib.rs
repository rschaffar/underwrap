//! In-memory Secret Service state backed by a JSON envelope.
//!
//! This crate provides the data model and serialization for the
//! freedesktop Secret Service state that `underwrap-secretsd` manages.
//! The entire state (collections, items, attributes, labels, timestamps,
//! AND secret values) is stored in a single serializable struct.
//!
//! **Invariant**: The JSON envelope contains secret values. It is NEVER
//! written to a local file. It is only serialized/deserialized over the
//! daemon channel (authenticated socketpair fd) or in tests.
//!
//! The store is intentionally simple: search is O(n) over items in a
//! collection. This is fine because each wrapper has at most a handful
//! of libsecret items.

mod store;

pub use store::{Collection, Item, SecretStore};
