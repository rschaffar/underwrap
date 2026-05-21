//! Backend-managed store registry.
//!
//! The on-disk registry at `BackendConfig::stores_file` lists every
//! store this backend instance owns. At startup the file is loaded
//! and each entry is turned into a [`StoreState`] — plain stores boot
//! `Unlocked`, encrypted stores boot `Locked` until an operator
//! submits the master key. The live `StoreRegistry` is shared (under
//! an `RwLock`) with every admin / runtime handler so lock-state
//! transitions take effect immediately.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

use underwrap_protocol::config::{StoreRecord, StoreRegistryDocument};
use underwrap_store::StorageBackend;

/// One store as loaded from the backend-managed store registry and
/// opened locally.
#[derive(Debug, Clone)]
pub struct LoadedStoreRecord {
    pub record: StoreRecord,
}

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
    pub fn is_locked(&self) -> bool {
        matches!(self, StoreState::Locked)
    }

    pub fn backend(&self) -> Option<&Arc<dyn StorageBackend>> {
        match self {
            StoreState::Unlocked(b) => Some(b),
            StoreState::Locked => None,
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

/// Open backends + provenance metadata for every store visible to the
/// admin API. Wrapped in an `RwLock` inside [`crate::runtime_state::AdminRuntimeState`]
/// so the CRUD handlers can mutate it without rebooting the daemon.
/// Readers consult [`StoreRegistry::unlocked_backend`] to fetch the
/// live `Arc<dyn StorageBackend>` for a store and surface
/// [`RegistryLookupError::Locked`] when the store hasn't been
/// unlocked yet.
pub struct StoreRegistry {
    pub state: HashMap<String, StoreState>,
    pub records: HashMap<String, LoadedStoreRecord>,
}

impl StoreRegistry {
    pub fn new() -> Self {
        Self {
            state: HashMap::new(),
            records: HashMap::new(),
        }
    }

    /// Return the live backend for `store_id` if it is unlocked, or a
    /// structured error otherwise. Used by every service / admin
    /// handler that needs to touch backing storage.
    pub fn unlocked_backend(
        &self,
        store_id: &str,
    ) -> std::result::Result<Arc<dyn StorageBackend>, RegistryLookupError> {
        match self.state.get(store_id) {
            Some(StoreState::Unlocked(b)) => Ok(Arc::clone(b)),
            Some(StoreState::Locked) => Err(RegistryLookupError::Locked),
            None => Err(RegistryLookupError::NotFound),
        }
    }

    /// `true` when the store exists in the registry and is currently
    /// in the locked state. `false` for plain stores, for unlocked
    /// encrypted stores, and for unknown ids.
    pub fn is_locked(&self, store_id: &str) -> bool {
        matches!(self.state.get(store_id), Some(StoreState::Locked))
    }

    /// Install an unlocked backend for `store_id`. Used after a
    /// successful unlock or by store creation. Returns the previous
    /// state so the caller can decide what to do (e.g. error out on
    /// `Unlocked` to avoid a race on already-unlocked stores).
    pub fn unlock(
        &mut self,
        store_id: &str,
        backend: Arc<dyn StorageBackend>,
    ) -> Option<StoreState> {
        self.state
            .insert(store_id.to_string(), StoreState::Unlocked(backend))
    }

    /// Transition `store_id` to the locked state, dropping any held
    /// backend. Returns `true` when the store existed in the registry.
    pub fn lock(&mut self, store_id: &str) -> bool {
        if self.state.contains_key(store_id) {
            self.state.insert(store_id.to_string(), StoreState::Locked);
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
        match self.state.get(store_id) {
            Some(StoreState::Unlocked(b)) => Arc::clone(b),
            Some(StoreState::Locked) => panic!("expected '{store_id}' to be unlocked"),
            None => panic!("expected '{store_id}' in registry"),
        }
    }
}

impl Default for StoreRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the initial [`StoreState`] map for the on-disk store
/// registry. Plain stores are opened immediately and inserted as
/// [`StoreState::Unlocked`]. Encrypted stores are inserted as
/// [`StoreState::Locked`] — the backend does not hold their master
/// key, so they stay locked until an operator submits the key via
/// `POST /v1/admin/stores/{id}/unlock`.
pub(crate) fn open_initial_stores(
    definitions: &HashMap<String, LoadedStoreRecord>,
) -> Result<HashMap<String, StoreState>> {
    let mut stores = HashMap::new();
    for (id, def) in definitions {
        let state = match &def.record.storage {
            underwrap_protocol::config::StorageConfig::Plain { .. } => {
                let backend: Arc<dyn StorageBackend> =
                    Arc::from(underwrap_store::open(&def.record.storage)?);
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
        stores.insert(id.clone(), state);
    }
    Ok(stores)
}

/// Read the backend-managed store-registry file and return the runtime
/// store map keyed by stable store id. Missing files are treated as an
/// empty registry so a fresh backend can still boot.
pub fn load_store_records(stores_file: &Path) -> Result<HashMap<String, LoadedStoreRecord>> {
    let text = match std::fs::read_to_string(stores_file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", stores_file.display())),
    };
    let doc: StoreRegistryDocument = serde_json::from_str(&text)
        .with_context(|| format!("parsing store registry {}", stores_file.display()))?;
    anyhow::ensure!(
        doc.version == 1,
        "unsupported store registry version {}",
        doc.version
    );
    let mut out: HashMap<String, LoadedStoreRecord> = HashMap::new();
    let mut names = std::collections::BTreeSet::new();
    for record in doc.stores {
        if !is_valid_store_id(&record.id) {
            anyhow::bail!(
                "invalid store id '{}' in {}: must match [a-z0-9][a-z0-9_-]*",
                record.id,
                stores_file.display()
            );
        }
        let trimmed_name = record.name.trim();
        anyhow::ensure!(
            !trimmed_name.is_empty(),
            "store '{}' in {} has an empty visible name",
            record.id,
            stores_file.display()
        );
        let name_key = trimmed_name.to_ascii_lowercase();
        anyhow::ensure!(
            names.insert(name_key),
            "duplicate visible store name '{}' in {}",
            trimmed_name,
            stores_file.display()
        );
        if out
            .insert(record.id.clone(), LoadedStoreRecord { record })
            .is_some()
        {
            anyhow::bail!("duplicate store id in {}", stores_file.display());
        }
    }
    Ok(out)
}

pub(crate) fn save_store_records(
    stores_file: &Path,
    records: &HashMap<String, LoadedStoreRecord>,
) -> Result<()> {
    let mut stores: Vec<StoreRecord> = records.values().map(|r| r.record.clone()).collect();
    stores.sort_by(|a, b| a.id.cmp(&b.id));
    let doc = StoreRegistryDocument { version: 1, stores };
    let json = serde_json::to_vec_pretty(&doc).context("serialising store registry")?;
    let dir = stores_file.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "stores file path '{}' has no parent directory",
            stores_file.display()
        )
    })?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating store-registry dir {}", dir.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("creating temp store-registry under {}", dir.display()))?;
    use std::io::Write;
    tmp.write_all(&json)
        .context("writing store-registry tempfile")?;
    tmp.as_file()
        .sync_all()
        .context("fsync store-registry tempfile")?;
    tmp.persist(stores_file).map_err(|e| {
        anyhow::anyhow!(
            "atomically persisting store registry to {}: {e}",
            stores_file.display(),
        )
    })?;
    Ok(())
}

/// A store id must be safe to use as a filename and as a URL path
/// segment. Lowercase letters, digits, `-`, `_`, starting with an
/// alphanumeric.
pub fn is_valid_store_id(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub(crate) fn validate_store_name(name: &str) -> std::result::Result<String, &'static str> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        Err("store name must not be empty")
    } else {
        Ok(trimmed.to_string())
    }
}

pub(crate) fn generate_store_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 10];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!(
        "store_{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

#[cfg(test)]
mod tests;
