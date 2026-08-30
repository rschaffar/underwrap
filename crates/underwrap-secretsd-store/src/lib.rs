//! In-memory Secret Service state model for one `underwrap-secretsd` process.
//!
//! The model holds collections, items, aliases, timestamps, and raw secret bytes.
//! Its in-process operations use operating-system time and randomness.
//! This crate performs no file or socket I/O, canonicalization, publication,
//! acknowledgement, backend access, or persistence I/O.
//!
//! The production `underwrap-secretsd` converts its one-shot initialized canonical
//! `LibsecretSection` into this store and converts candidate stores back into
//! sections for private publication.
//! It commits a candidate only after an external acknowledgement.
//! Callers own lifecycle, transport, persistence, and erasure policy.
//!
//! The model represents secret values as raw bytes and provides no memory-erasure
//! guarantee.
//!
//! Search is intentionally O(n) over items in a collection because each wrapper
//! has at most a handful of libsecret items.

mod store;

pub use store::{Collection, Item, SecretStore};
