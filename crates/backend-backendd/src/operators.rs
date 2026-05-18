//! Operator records: the people who can log in to the admin UI.
//!
//! Records live in a backend-managed JSON file at
//! [`DEFAULT_OPERATORS_FILE`] by default. The same document also holds
//! the persistent bootstrap-completed sentinel. Subsequent tickets
//! (211 login, 213 role middleware, 214 bootstrap, 240 user
//! management) consume this module.
//!
//! Password hashes are Argon2id PHC strings. We never log them; the
//! [`OperatorRecord`] `Debug` impl redacts them and tests assert that
//! tracing-captured CRUD output never contains the hash material.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Default location of the backend-managed operator-state file.
pub const DEFAULT_OPERATORS_FILE: &str = "/var/lib/underwrap-backend/operators.json";

/// Schema version for the persisted operator collection. Bump when the
/// on-disk shape changes incompatibly.
pub const COLLECTION_VERSION: u32 = 1;

/// Schema version for the persisted operator-state file.
const STATE_FILE_VERSION: u32 = 1;

/// One operator account.
///
/// `password_hash` is an Argon2id PHC string. It must never appear in
/// logs; the custom `Debug` impl below redacts it.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperatorRecord {
    /// Stable identifier. Random 16-byte hex string, generated server-side.
    pub id: String,
    /// Lowercase, trimmed. Unique across the collection.
    pub username: String,
    /// Argon2id PHC-encoded password hash.
    pub password_hash: String,
    pub role: OperatorRole,
    #[serde(default)]
    pub granted_stores: Vec<String>,
    pub created_at: u64,
    pub password_changed_at: u64,
}

impl std::fmt::Debug for OperatorRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperatorRecord")
            .field("id", &self.id)
            .field("username", &self.username)
            .field("password_hash", &"<redacted>")
            .field("role", &self.role)
            .field("granted_stores", &self.granted_stores)
            .field("created_at", &self.created_at)
            .field("password_changed_at", &self.password_changed_at)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum OperatorRole {
    Admin,
    Operator,
}

/// Resolved identity and authorization context for an admin request.
/// Derived from either a session cookie (looked up in the operator
/// collection at request time) or a legacy bearer token. Carries
/// enough information to gate per-store access without re-reading the
/// collection on every authorization decision.
#[derive(Debug, Clone)]
pub struct Principal {
    pub id: String,
    pub username: String,
    pub role: OperatorRole,
    pub granted_stores: Vec<String>,
}

impl Principal {
    pub fn is_admin(&self) -> bool {
        matches!(self.role, OperatorRole::Admin)
    }

    /// Admins can access any store; operators must have an explicit
    /// grant. We don't distinguish "no grant" from "no such store"
    /// at the response level — both return 404 to avoid leaking
    /// store existence to non-admins.
    pub fn has_store_access(&self, store: &str) -> bool {
        self.is_admin() || self.granted_stores.iter().any(|s| s == store)
    }
}

/// The in-memory operator collection.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperatorCollection {
    pub version: u32,
    pub operators: Vec<OperatorRecord>,
}

impl std::fmt::Debug for OperatorCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `OperatorRecord`'s Debug impl already redacts the hash; we
        // implement Debug here to keep the same redaction-by-default
        // posture for the wrapper.
        f.debug_struct("OperatorCollection")
            .field("version", &self.version)
            .field("operators", &self.operators)
            .finish()
    }
}

impl Default for OperatorCollection {
    fn default() -> Self {
        Self {
            version: COLLECTION_VERSION,
            operators: Vec::new(),
        }
    }
}

impl OperatorCollection {
    /// Number of operator records currently stored.
    pub fn len(&self) -> usize {
        self.operators.len()
    }

    /// True when no operator records exist. Used by the bootstrap flow
    /// in ticket 214 to decide whether to show the bootstrap page.
    pub fn is_empty(&self) -> bool {
        self.operators.is_empty()
    }

    /// Look up an operator by username. Username comparison is case-
    /// insensitive after the per-record `username` has been normalised
    /// on insert.
    pub fn find_by_username(&self, username: &str) -> Option<&OperatorRecord> {
        let needle = normalise_username(username);
        self.operators.iter().find(|o| o.username == needle)
    }

    /// Append a record. Refuses duplicate usernames.
    pub fn add(&mut self, mut record: OperatorRecord) -> Result<()> {
        record.username = normalise_username(&record.username);
        if record.username.is_empty() {
            anyhow::bail!("operator username must be non-empty");
        }
        if !record.password_hash.starts_with("$argon2") {
            anyhow::bail!("operator password_hash must be a PHC-encoded argon2 hash");
        }
        if self.find_by_username(&record.username).is_some() {
            anyhow::bail!("operator '{}' already exists", record.username);
        }
        self.operators.push(record);
        Ok(())
    }

    /// Look up an operator by id.
    pub fn find_by_id(&self, id: &str) -> Option<&OperatorRecord> {
        self.operators.iter().find(|o| o.id == id)
    }

    /// Number of operators currently holding the [`Admin`](OperatorRole::Admin) role.
    pub fn admin_count(&self) -> usize {
        self.operators
            .iter()
            .filter(|o| matches!(o.role, OperatorRole::Admin))
            .count()
    }

    /// Replace the record with id `id` using the result of `f`. The
    /// closure receives a mutable reference and may rename / re-role /
    /// re-grant the record; uniqueness, last-admin, and PHC-hash
    /// invariants are re-checked here so callers don't have to.
    pub fn update<F>(&mut self, id: &str, f: F) -> Result<&OperatorRecord>
    where
        F: FnOnce(&mut OperatorRecord),
    {
        let idx = self
            .operators
            .iter()
            .position(|o| o.id == id)
            .with_context(|| format!("operator '{id}' not found"))?;
        let before = self.operators[idx].clone();
        let mut next = before.clone();
        f(&mut next);
        next.username = normalise_username(&next.username);
        if next.username.is_empty() {
            anyhow::bail!("operator username must be non-empty");
        }
        if !next.password_hash.starts_with("$argon2") {
            anyhow::bail!("operator password_hash must be a PHC-encoded argon2 hash");
        }
        // Reject username conflicts with any *other* operator.
        if let Some(other) = self.find_by_username(&next.username)
            && other.id != next.id
        {
            anyhow::bail!("operator '{}' already exists", next.username);
        }
        // Last-admin protection: if the operator was an admin and the
        // proposed role isn't admin, the new collection must still have
        // at least one admin.
        if matches!(before.role, OperatorRole::Admin)
            && !matches!(next.role, OperatorRole::Admin)
            && self.admin_count() <= 1
        {
            anyhow::bail!("last_admin_protected");
        }
        self.operators[idx] = next;
        Ok(&self.operators[idx])
    }

    /// Remove the operator with id `id`. Refuses to remove the last
    /// admin (returns an error whose `to_string()` is exactly
    /// `last_admin_protected`).
    pub fn remove(&mut self, id: &str) -> Result<OperatorRecord> {
        let idx = self
            .operators
            .iter()
            .position(|o| o.id == id)
            .with_context(|| format!("operator '{id}' not found"))?;
        if matches!(self.operators[idx].role, OperatorRole::Admin) && self.admin_count() <= 1 {
            anyhow::bail!("last_admin_protected");
        }
        Ok(self.operators.remove(idx))
    }
}

/// Persistent marker that the online bootstrap flow has completed.
/// Stored alongside the operator collection in the operator-state file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapSentinel {
    pub completed_at: u64,
    pub by_username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct OperatorStateDocument {
    #[serde(default = "default_state_file_version")]
    version: u32,
    #[serde(default)]
    operators: Vec<OperatorRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bootstrap_completed: Option<BootstrapSentinel>,
}

impl Default for OperatorStateDocument {
    fn default() -> Self {
        Self {
            version: STATE_FILE_VERSION,
            operators: Vec::new(),
            bootstrap_completed: None,
        }
    }
}

fn default_state_file_version() -> u32 {
    STATE_FILE_VERSION
}

/// Normalise a username: trimmed, lowercase. Usernames are compared
/// case-insensitively because operators routinely confuse `Alice` and
/// `alice` between admin set-up and login.
pub fn normalise_username(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Generate a new opaque operator id. 16 random bytes hex-encoded, so
/// it looks UUID-shaped without pulling in the `uuid` crate.
pub fn new_operator_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Current epoch seconds, saturating to 0 on the very-unlikely
/// pre-1970 clock case.
pub fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Load the operator collection from the backend-managed operator-state
/// file. Returns the default (empty) collection when no file exists
/// yet; the caller treats that as "bootstrap required".
pub fn load(path: &Path) -> Result<OperatorCollection> {
    let doc = load_state_document(path)?;
    Ok(OperatorCollection {
        version: COLLECTION_VERSION,
        operators: doc.operators,
    })
}

/// Persist the operator collection while preserving the bootstrap
/// sentinel already present in the operator-state file.
pub fn save(path: &Path, collection: &OperatorCollection) -> Result<()> {
    anyhow::ensure!(
        collection.version == COLLECTION_VERSION,
        "operator collection has version {}, expected {}",
        collection.version,
        COLLECTION_VERSION
    );
    let mut doc = load_state_document(path)?;
    doc.operators = collection.operators.clone();
    save_state_document(path, &doc)
}

/// True iff the bootstrap-completed sentinel is present.
pub fn bootstrap_completed(path: &Path) -> Result<bool> {
    Ok(load_state_document(path)?.bootstrap_completed.is_some())
}

/// Read the sentinel record, if present.
pub fn load_bootstrap_sentinel(path: &Path) -> Result<Option<BootstrapSentinel>> {
    Ok(load_state_document(path)?.bootstrap_completed)
}

/// Persist the bootstrap-completed sentinel while preserving the
/// operator collection already present in the operator-state file.
pub fn mark_bootstrap_completed(path: &Path, sentinel: &BootstrapSentinel) -> Result<()> {
    let mut doc = load_state_document(path)?;
    doc.bootstrap_completed = Some(sentinel.clone());
    save_state_document(path, &doc)
}

fn load_state_document(path: &Path) -> Result<OperatorStateDocument> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(OperatorStateDocument::default());
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let doc: OperatorStateDocument = serde_json::from_str(&text)
        .with_context(|| format!("parsing operator-state file {}", path.display()))?;
    anyhow::ensure!(
        doc.version == STATE_FILE_VERSION,
        "operator-state file has version {}, expected {}",
        doc.version,
        STATE_FILE_VERSION
    );
    Ok(doc)
}

fn save_state_document(path: &Path, doc: &OperatorStateDocument) -> Result<()> {
    anyhow::ensure!(
        doc.version == STATE_FILE_VERSION,
        "operator-state file has version {}, expected {}",
        doc.version,
        STATE_FILE_VERSION
    );
    let json = serde_json::to_vec_pretty(doc).context("serialising operator-state file")?;
    let dir = path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "operator-state file path '{}' has no parent directory",
            path.display()
        )
    })?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating operator-state dir {}", dir.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("creating temp operator-state file under {}", dir.display()))?;
    use std::io::Write;
    tmp.write_all(&json)
        .with_context(|| format!("writing temp operator-state file under {}", dir.display()))?;
    tmp.flush()
        .with_context(|| format!("flushing temp operator-state file under {}", dir.display()))?;
    tmp.persist(path).map_err(|e| {
        anyhow::anyhow!(
            "atomically persisting operator-state file to {}: {e}",
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record(username: &str, role: OperatorRole) -> OperatorRecord {
        OperatorRecord {
            id: new_operator_id(),
            username: username.to_string(),
            password_hash:
                "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
                    .to_string(),
            role,
            granted_stores: Vec::new(),
            created_at: 1000,
            password_changed_at: 1000,
        }
    }

    fn test_state_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "underwrap-operator-state-test-{}.json",
            new_operator_id()
        ))
    }

    #[test]
    fn collection_round_trips_through_serde() {
        let mut c = OperatorCollection::default();
        c.add(sample_record("alice", OperatorRole::Admin)).unwrap();
        c.add(sample_record("bob", OperatorRole::Operator)).unwrap();
        let bytes = serde_json::to_vec(&c).unwrap();
        let parsed: OperatorCollection = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn add_rejects_duplicate_username_case_insensitively() {
        let mut c = OperatorCollection::default();
        c.add(sample_record("Alice", OperatorRole::Admin)).unwrap();
        let err = c
            .add(sample_record("alice", OperatorRole::Operator))
            .unwrap_err();
        assert!(err.to_string().contains("already exists"));
        // Username has been normalised on insert.
        assert_eq!(c.operators[0].username, "alice");
    }

    #[test]
    fn add_rejects_empty_username() {
        let mut c = OperatorCollection::default();
        let err = c
            .add(sample_record("   ", OperatorRole::Admin))
            .unwrap_err();
        assert!(err.to_string().contains("non-empty"));
    }

    #[test]
    fn add_rejects_non_argon2_hash() {
        let mut c = OperatorCollection::default();
        let mut bad = sample_record("alice", OperatorRole::Admin);
        bad.password_hash = "plaintext".to_string();
        let err = c.add(bad).unwrap_err();
        assert!(err.to_string().contains("argon2"));
    }

    #[test]
    fn find_by_username_is_case_insensitive() {
        let mut c = OperatorCollection::default();
        c.add(sample_record("Alice", OperatorRole::Admin)).unwrap();
        assert!(c.find_by_username("alice").is_some());
        assert!(c.find_by_username("ALICE").is_some());
        assert!(c.find_by_username("  alice  ").is_some());
        assert!(c.find_by_username("bob").is_none());
    }

    #[test]
    fn empty_collection_signals_bootstrap_required() {
        let path = test_state_path();
        let collection = load(&path).unwrap();
        assert!(collection.is_empty(), "fresh state yields empty collection");
    }

    #[test]
    fn save_and_load_round_trip() {
        let path = test_state_path();
        let mut c = OperatorCollection::default();
        c.add(sample_record("alice", OperatorRole::Admin)).unwrap();
        save(&path, &c).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded, c);
    }

    #[test]
    fn load_rejects_unknown_state_file_version() {
        let path = test_state_path();
        std::fs::write(&path, r#"{"version":99,"operators":[]}"#).unwrap();
        let err = load(&path).unwrap_err();
        assert!(err.to_string().contains("version"));
    }

    #[test]
    fn bootstrap_sentinel_absent_by_default() {
        let path = test_state_path();
        assert!(!bootstrap_completed(&path).unwrap());
        assert!(load_bootstrap_sentinel(&path).unwrap().is_none());
    }

    #[test]
    fn bootstrap_sentinel_round_trip() {
        let path = test_state_path();
        let sentinel = BootstrapSentinel {
            completed_at: 1234,
            by_username: "alice".into(),
        };
        mark_bootstrap_completed(&path, &sentinel).unwrap();
        assert!(bootstrap_completed(&path).unwrap());
        let loaded = load_bootstrap_sentinel(&path).unwrap().unwrap();
        assert_eq!(loaded, sentinel);
    }

    #[test]
    fn saving_collection_preserves_existing_bootstrap_sentinel() {
        let path = test_state_path();
        let sentinel = BootstrapSentinel {
            completed_at: 1234,
            by_username: "alice".into(),
        };
        mark_bootstrap_completed(&path, &sentinel).unwrap();
        let mut c = OperatorCollection::default();
        c.add(sample_record("alice", OperatorRole::Admin)).unwrap();
        save(&path, &c).unwrap();
        assert_eq!(load(&path).unwrap(), c);
        assert_eq!(load_bootstrap_sentinel(&path).unwrap().unwrap(), sentinel);
    }

    #[test]
    fn principal_admin_has_access_to_every_store() {
        let p = Principal {
            id: "a".into(),
            username: "alice".into(),
            role: OperatorRole::Admin,
            granted_stores: Vec::new(),
        };
        assert!(p.is_admin());
        assert!(p.has_store_access("anything"));
        assert!(p.has_store_access("else"));
    }

    #[test]
    fn principal_operator_requires_explicit_grant() {
        let p = Principal {
            id: "b".into(),
            username: "bob".into(),
            role: OperatorRole::Operator,
            granted_stores: vec!["work".into()],
        };
        assert!(!p.is_admin());
        assert!(p.has_store_access("work"));
        assert!(!p.has_store_access("other"));
    }

    #[test]
    fn debug_redacts_password_hash() {
        let r = sample_record("alice", OperatorRole::Admin);
        let dbg = format!("{r:?}");
        assert!(
            !dbg.contains("argon2"),
            "Debug must not leak the PHC string"
        );
        assert!(dbg.contains("<redacted>"));
        // Same applies inside a collection.
        let mut c = OperatorCollection::default();
        c.add(r).unwrap();
        let dbg = format!("{c:?}");
        assert!(!dbg.contains("argon2"));
    }
}
