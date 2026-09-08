//! Backend-managed store registry and scoped normal-storage leases.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Notify;
use tracing::info;

use underwrap_backend_store::StorageBackend;
use underwrap_protocol::config::StoreRecord;

/// Live lifecycle state of one store.
pub enum StoreState {
    Ready(Arc<dyn StorageBackend>),
    Unlocked(Arc<dyn StorageBackend>),
    Locked,
    Busy,
    RecoveryRequired,
}

impl StoreState {
    #[cfg(test)]
    pub(crate) fn backend(&self) -> Option<&Arc<dyn StorageBackend>> {
        match self {
            Self::Ready(backend) | Self::Unlocked(backend) => Some(backend),
            Self::Locked | Self::Busy | Self::RecoveryRequired => None,
        }
    }
}

/// Entry-owned admission count for complete normal storage operations.
pub(crate) struct LeaseTracker {
    count: AtomicUsize,
    notify: Notify,
}

impl LeaseTracker {
    fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
            notify: Notify::new(),
        }
    }

    fn admit(&self) {
        self.count.fetch_add(1, Ordering::AcqRel);
    }

    fn release(&self) {
        let previous = self.count.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "lease count underflow");
        if previous == 1 {
            // One lifecycle mutex owner can wait for this entry at a time.
            // `notify_one` retains a permit if release races waiter registration.
            self.notify.notify_one();
        }
    }

    pub(crate) fn count(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }

    pub(crate) async fn wait_for_zero(&self, timeout: Duration) -> bool {
        tokio::time::timeout(timeout, async {
            loop {
                let notified = self.notify.notified();
                if self.count() == 0 {
                    break;
                }
                notified.await;
            }
        })
        .await
        .is_ok()
    }
}

/// A backend handle whose lifetime covers one complete handler storage operation.
pub(crate) struct StoreLease {
    backend: Arc<dyn StorageBackend>,
    tracker: Arc<LeaseTracker>,
}

impl Deref for StoreLease {
    type Target = dyn StorageBackend;

    fn deref(&self) -> &Self::Target {
        self.backend.as_ref()
    }
}

impl AsRef<dyn StorageBackend> for StoreLease {
    fn as_ref(&self) -> &(dyn StorageBackend + 'static) {
        self.backend.as_ref()
    }
}

impl Drop for StoreLease {
    fn drop(&mut self) {
        self.tracker.release();
    }
}

/// One store as held by the live registry.
pub struct StoreEntry {
    pub record: StoreRecord,
    pub state: StoreState,
    pub lifecycle_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) identity: Arc<()>,
    pub(crate) generation: AtomicU64,
    pub(crate) leases: Arc<LeaseTracker>,
}

impl StoreEntry {
    pub fn new(record: StoreRecord, state: StoreState) -> Self {
        Self {
            record,
            state,
            lifecycle_lock: Arc::new(tokio::sync::Mutex::new(())),
            identity: Arc::new(()),
            generation: AtomicU64::new(0),
            leases: Arc::new(LeaseTracker::new()),
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub(crate) fn advance_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryLookupError {
    NotFound,
    Locked,
    Busy,
    RecoveryRequired,
}

#[derive(Default)]
pub struct StoreRegistry {
    pub entries: HashMap<String, StoreEntry>,
}

impl StoreRegistry {
    pub fn entry(&self, store_id: &str) -> std::result::Result<&StoreEntry, RegistryLookupError> {
        self.entries
            .get(store_id)
            .ok_or(RegistryLookupError::NotFound)
    }

    /// Admit one complete normal storage operation while the registry lock is held.
    pub(crate) fn lease(
        &self,
        store_id: &str,
    ) -> std::result::Result<StoreLease, RegistryLookupError> {
        let entry = self.entry(store_id)?;
        let backend = match &entry.state {
            StoreState::Ready(backend) | StoreState::Unlocked(backend) => Arc::clone(backend),
            StoreState::Locked => return Err(RegistryLookupError::Locked),
            StoreState::Busy => return Err(RegistryLookupError::Busy),
            StoreState::RecoveryRequired => return Err(RegistryLookupError::RecoveryRequired),
        };
        entry.leases.admit();
        Ok(StoreLease {
            backend,
            tracker: Arc::clone(&entry.leases),
        })
    }

    pub fn unlock(&mut self, store_id: &str, backend: Arc<dyn StorageBackend>) {
        if let Some(entry) = self.entries.get_mut(store_id) {
            entry.state = StoreState::Unlocked(backend);
            entry.advance_generation();
        }
    }

    pub fn lock(&mut self, store_id: &str) -> bool {
        if let Some(entry) = self.entries.get_mut(store_id) {
            entry.state = StoreState::Locked;
            entry.advance_generation();
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub fn expect_unlocked(&self, store_id: &str) -> Arc<dyn StorageBackend> {
        match self.entries.get(store_id) {
            Some(entry) => match &entry.state {
                StoreState::Ready(backend) | StoreState::Unlocked(backend) => Arc::clone(backend),
                StoreState::Locked | StoreState::Busy | StoreState::RecoveryRequired => {
                    panic!("expected '{store_id}' to be available")
                }
            },
            None => panic!("expected '{store_id}' in registry"),
        }
    }
}

fn has_exact_migration_workspace(store_id: &str, canonical: &Path) -> Result<bool> {
    let Some(parent) = canonical.parent() else {
        return Ok(false);
    };
    let entries = match std::fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("listing store data parent {}", parent.display()));
        }
    };
    let prefix = format!(".{store_id}.underwrap-migration-");
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(suffix) = name.strip_prefix(&prefix) else {
            continue;
        };
        if suffix.len() == 16
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Build initial entries without mutating any store that has durable migration state.
pub(crate) fn open_initial_stores(
    definitions: HashMap<String, StoreRecord>,
) -> Result<HashMap<String, StoreEntry>> {
    let mut entries = HashMap::new();
    for (id, record) in definitions {
        let state = if has_exact_migration_workspace(&id, record.storage.dir())? {
            info!(store = %id, "store has migration workspace; trusted-host recovery required");
            StoreState::RecoveryRequired
        } else {
            match &record.storage {
                underwrap_protocol::config::StorageConfig::Plain { .. } => {
                    let backend: Arc<dyn StorageBackend> =
                        Arc::from(underwrap_backend_store::open(&record.storage)?);
                    StoreState::Ready(backend)
                }
                underwrap_protocol::config::StorageConfig::Encrypted { .. } => {
                    info!(
                        store = %id,
                        "encrypted store starts locked; awaiting POST /v1/admin/stores/{{id}}/unlock",
                    );
                    StoreState::Locked
                }
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

pub fn load_store_records(dir: &Path) -> Result<HashMap<String, StoreRecord>> {
    let records: Vec<StoreRecord> = crate::storage::json_dir_store::load_dir(dir)?;
    Ok(records
        .into_iter()
        .map(|record| (record.id.clone(), record))
        .collect())
}

pub fn save_store_record(dir: &Path, record: &StoreRecord) -> Result<()> {
    crate::storage::json_dir_store::save(dir, record)
}

pub fn delete_store_record(dir: &Path, store_id: &str) -> Result<()> {
    crate::storage::json_dir_store::delete(dir, "store", store_id)
}

#[cfg(test)]
mod tests;
