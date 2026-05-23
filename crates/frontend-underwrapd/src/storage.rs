//! Storage — delegates to underwrap-store backends.

pub use underwrap_store::{StorageBackend, open};

#[cfg(test)]
mod tests;
