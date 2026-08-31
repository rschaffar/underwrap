//! Backend-managed store registry.
//!
//! The on-disk registry at `BackendConfig::stores_dir` holds one
//! `<id>.json` file per store this backend instance owns. At startup
//! every file is loaded and each entry is turned into a [`StoreEntry`]
//! — plain stores boot `Unlocked`, encrypted stores boot `Locked`
//! until an operator submits the master key. The live `StoreRegistry`
//! is shared (under an `RwLock`) with every admin / runtime handler so
//! lock-state transitions take effect immediately.
//!
//! Writes go through per-record tempfile + atomic rename; deletes are
//! a single unlink. The id lives only in the filename, so renaming a
//! store on disk is one `mv`, not a rewrite of a master document.

use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

use underwrap_backend_store::StorageBackend;
use underwrap_protocol::config::StoreRecord;

/// Live lock-state of one store. Plain stores are constructed as
/// [`StoreState::Unlocked`] at startup and stay there for the
/// lifetime of the process. Encrypted stores boot as
/// [`StoreState::Locked`] and only transition to `Unlocked` after an
/// operator submits the master key via `POST .../unlock`. Dropping the
/// `Arc<dyn StorageBackend>` zeroizes the held `MasterKey`.
pub enum StoreState {
    Unlocked(Arc<dyn StorageBackend>),
    Locked,
}

impl StoreState {
    pub fn backend(&self) -> Option<&Arc<dyn StorageBackend>> {
        match self {
            StoreState::Unlocked(b) => Some(b),
            StoreState::Locked => None,
        }
    }
}

/// One store as held by the live registry: persisted record, current
/// lock state, and a mutex that serializes rotation requests with one
/// another. Ordinary reads and writes do not acquire this mutex; operators
/// must quiesce store access during rotation. The lock lives on the entry so
/// it dies with the store on delete instead of leaking into an unbounded map
/// on `ListenerState`.
pub struct StoreEntry {
    pub record: StoreRecord,
    pub state: StoreState,
    pub rotation_lock: Arc<tokio::sync::Mutex<()>>,
}

impl StoreEntry {
    pub fn new(record: StoreRecord, state: StoreState) -> Self {
        Self {
            record,
            state,
            rotation_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

/// Reason a `StoreRegistry` lookup failed. Translated to HTTP at the
/// handler boundary: `NotFound → 404`, `Locked → 503 / store_locked`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryLookupError {
    NotFound,
    Locked,
}

/// Open backends + provenance metadata + per-store rotation mutexes for every
/// store visible to the admin API. Wrapped in an `RwLock` inside
/// `ListenerState` so the CRUD handlers
/// can mutate it without rebooting the daemon. Readers consult
/// [`StoreRegistry::unlocked_backend`] to fetch the live
/// `Arc<dyn StorageBackend>` for a store and surface
/// [`RegistryLookupError::Locked`] when the store hasn't been
/// unlocked yet.
#[derive(Default)]
pub struct StoreRegistry {
    pub entries: HashMap<String, StoreEntry>,
}

impl StoreRegistry {
    /// Borrow the live entry for `store_id`, or surface
    /// [`RegistryLookupError::NotFound`] when no such store is
    /// registered. Callers needing to distinguish Locked vs Unlocked
    /// match on the returned entry's `state` field.
    pub fn entry(&self, store_id: &str) -> std::result::Result<&StoreEntry, RegistryLookupError> {
        self.entries
            .get(store_id)
            .ok_or(RegistryLookupError::NotFound)
    }

    /// Return the live backend for `store_id` if it is unlocked, or a
    /// structured error otherwise. Used by every service / admin
    /// handler that needs to touch backing storage.
    pub fn unlocked_backend(
        &self,
        store_id: &str,
    ) -> std::result::Result<Arc<dyn StorageBackend>, RegistryLookupError> {
        match &self.entry(store_id)?.state {
            StoreState::Unlocked(b) => Ok(Arc::clone(b)),
            StoreState::Locked => Err(RegistryLookupError::Locked),
        }
    }

    /// Install an unlocked backend for `store_id`. Used after a
    /// successful unlock. The entry must already exist.
    pub fn unlock(&mut self, store_id: &str, backend: Arc<dyn StorageBackend>) {
        if let Some(entry) = self.entries.get_mut(store_id) {
            entry.state = StoreState::Unlocked(backend);
        }
    }

    /// Transition `store_id` to the locked state, dropping any held
    /// backend. Returns `true` when the store existed in the registry.
    pub fn lock(&mut self, store_id: &str) -> bool {
        if let Some(entry) = self.entries.get_mut(store_id) {
            entry.state = StoreState::Locked;
            true
        } else {
            false
        }
    }

    /// Test-only: return the unlocked backend for `store_id`,
    /// panicking with a helpful message if the store is locked or
    /// unknown. Cuts down on noise in tests that already know the
    /// store is unlocked because they just created it.
    #[cfg(test)]
    pub fn expect_unlocked(&self, store_id: &str) -> Arc<dyn StorageBackend> {
        match self.entries.get(store_id) {
            Some(entry) => match &entry.state {
                StoreState::Unlocked(b) => Arc::clone(b),
                StoreState::Locked => panic!("expected '{store_id}' to be unlocked"),
            },
            None => panic!("expected '{store_id}' in registry"),
        }
    }
}

/// Build the initial [`StoreEntry`] map for the on-disk store
/// registry. Plain stores are opened immediately and inserted as
/// [`StoreState::Unlocked`]. Encrypted stores are inserted as
/// [`StoreState::Locked`] — the backend does not hold their master
/// key, so they stay locked until an operator submits the key via
/// `POST /v1/admin/stores/{id}/unlock`.
pub(crate) fn open_initial_stores(
    definitions: HashMap<String, StoreRecord>,
) -> Result<HashMap<String, StoreEntry>> {
    let mut entries = HashMap::new();
    for (id, record) in definitions {
        let state = match &record.storage {
            underwrap_protocol::config::StorageConfig::Plain { .. } => {
                let backend: Arc<dyn StorageBackend> =
                    Arc::from(underwrap_backend_store::open(&record.storage)?);
                StoreState::Unlocked(backend)
            }
            underwrap_protocol::config::StorageConfig::Encrypted { .. } => {
                info!(
                    store = %id,
                    "encrypted store starts locked; awaiting POST /v1/admin/stores/{{id}}/unlock",
                );
                StoreState::Locked
            }
        };
        entries.insert(id, StoreEntry::new(record, state));
    }
    Ok(entries)
}

impl crate::storage::json_dir_store::DirRecord for StoreRecord {
    const KIND: &'static str = "store";
    const KIND_PLURAL_DIR: &'static str = "stores directory";

    fn id(&self) -> &str {
        &self.id
    }

    fn set_id(&mut self, id: String) {
        self.id = id;
    }

    fn validate_fields(&self, path: &Path) -> Result<()> {
        anyhow::ensure!(
            !self.name.trim().is_empty(),
            "store '{}' has an empty visible name in {}",
            self.id,
            path.display(),
        );
        Ok(())
    }

    fn uniqueness_keys(&self) -> Vec<(&'static str, String)> {
        vec![("visible name", self.name.trim().to_ascii_lowercase())]
    }
}

/// Load every store record found under `dir`. A missing directory is
/// treated as an empty registry so a fresh backend can still boot.
pub fn load_store_records(dir: &Path) -> Result<HashMap<String, StoreRecord>> {
    let records: Vec<StoreRecord> = crate::storage::json_dir_store::load_dir(dir)?;
    Ok(records.into_iter().map(|r| (r.id.clone(), r)).collect())
}

/// Persist one store record to `<dir>/<id>.json` via the usual
/// tempfile + atomic rename.
pub fn save_store_record(dir: &Path, record: &StoreRecord) -> Result<()> {
    crate::storage::json_dir_store::save(dir, record)
}

/// Remove `<dir>/<id>.json`. Missing files are tolerated so the admin
/// delete path remains idempotent under retry.
pub fn delete_store_record(dir: &Path, store_id: &str) -> Result<()> {
    crate::storage::json_dir_store::delete(dir, "store", store_id)
}

#[cfg(test)]
mod tests;
