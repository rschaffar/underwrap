//! User records: the people who can log in to the admin UI.
//!
//! Records live in a backend-managed JSON file at
//! [`DEFAULT_USERS_FILE`] by default. The same document also holds the
//! persistent bootstrap-completed sentinel.
//!
//! Authorisation is intentionally flat: any authenticated user can
//! perform any admin action. The "last user" guard prevents the
//! collection from going empty (which would re-trigger the bootstrap
//! flow).
//!
//! Password hashes are Argon2id PHC strings. We never log them; the
//! [`UserRecord`] `Debug` impl redacts them and tests assert that
//! tracing-captured CRUD output never contains the hash material.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};

/// Default location of the backend-managed user-state file.
pub const DEFAULT_USERS_FILE: &str = "/var/lib/underwrap-backend/users.json";

/// Schema version for the persisted user collection. Bump when the
/// on-disk shape changes incompatibly.
pub const COLLECTION_VERSION: u32 = 2;

/// Schema version for the persisted user-state file.
const STATE_FILE_VERSION: u32 = 2;

/// One user account.
///
/// `password_hash` is an Argon2id PHC string. It must never appear in
/// logs; the custom `Debug` impl below redacts it.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserRecord {
    /// Stable identifier. Random 16-byte hex string, generated server-side.
    pub id: String,
    /// Lowercase, trimmed. Unique across the collection.
    pub username: String,
    /// Argon2id PHC-encoded password hash.
    pub password_hash: String,
    pub created_at: u64,
    pub password_changed_at: u64,
}

impl std::fmt::Debug for UserRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserRecord")
            .field("id", &self.id)
            .field("username", &self.username)
            .field("password_hash", &"<redacted>")
            .field("created_at", &self.created_at)
            .field("password_changed_at", &self.password_changed_at)
            .finish()
    }
}

/// Resolved identity for an admin request, derived from the
/// authenticated session cookie.
#[derive(Debug, Clone)]
pub struct Principal {
    pub id: String,
    pub username: String,
}

/// The in-memory user collection.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserCollection {
    pub version: u32,
    pub users: Vec<UserRecord>,
}

impl std::fmt::Debug for UserCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `UserRecord`'s Debug impl already redacts the hash; we
        // implement Debug here to keep the same redaction-by-default
        // posture for the wrapper.
        f.debug_struct("UserCollection")
            .field("version", &self.version)
            .field("users", &self.users)
            .finish()
    }
}

impl Default for UserCollection {
    fn default() -> Self {
        Self {
            version: COLLECTION_VERSION,
            users: Vec::new(),
        }
    }
}

impl UserCollection {
    pub fn len(&self) -> usize {
        self.users.len()
    }

    /// True when no user records exist. Used by the bootstrap flow
    /// to decide whether to show the bootstrap page.
    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }

    /// Look up a user by username. Username comparison is
    /// case-insensitive after the per-record `username` has been
    /// normalised on insert.
    pub fn find_by_username(&self, username: &str) -> Option<&UserRecord> {
        let needle = normalise_username(username);
        self.users.iter().find(|u| u.username == needle)
    }

    /// Append a record. Refuses duplicate usernames.
    pub fn add(&mut self, mut record: UserRecord) -> Result<()> {
        record.username = normalise_username(&record.username);
        if record.username.is_empty() {
            anyhow::bail!("user username must be non-empty");
        }
        if !record.password_hash.starts_with("$argon2") {
            anyhow::bail!("user password_hash must be a PHC-encoded argon2 hash");
        }
        if self.find_by_username(&record.username).is_some() {
            anyhow::bail!("user '{}' already exists", record.username);
        }
        self.users.push(record);
        Ok(())
    }

    /// Look up a user by id.
    pub fn find_by_id(&self, id: &str) -> Option<&UserRecord> {
        self.users.iter().find(|u| u.id == id)
    }

    /// Replace the record with id `id` using the result of `f`. The
    /// closure receives a mutable reference and may rename the record;
    /// uniqueness and PHC-hash invariants are re-checked here so
    /// callers don't have to.
    pub fn update<F>(&mut self, id: &str, f: F) -> Result<&UserRecord>
    where
        F: FnOnce(&mut UserRecord),
    {
        let idx = self
            .users
            .iter()
            .position(|u| u.id == id)
            .with_context(|| format!("user '{id}' not found"))?;
        let before = self.users[idx].clone();
        let mut next = before.clone();
        f(&mut next);
        next.username = normalise_username(&next.username);
        if next.username.is_empty() {
            anyhow::bail!("user username must be non-empty");
        }
        if !next.password_hash.starts_with("$argon2") {
            anyhow::bail!("user password_hash must be a PHC-encoded argon2 hash");
        }
        // Reject username conflicts with any *other* user.
        if let Some(other) = self.find_by_username(&next.username)
            && other.id != next.id
        {
            anyhow::bail!("user '{}' already exists", next.username);
        }
        self.users[idx] = next;
        Ok(&self.users[idx])
    }

    /// Remove the user with id `id`. Refuses to remove the last user
    /// (returns an error whose `to_string()` is exactly
    /// `last_user_protected`).
    pub fn remove(&mut self, id: &str) -> Result<UserRecord> {
        let idx = self
            .users
            .iter()
            .position(|u| u.id == id)
            .with_context(|| format!("user '{id}' not found"))?;
        if self.users.len() <= 1 {
            anyhow::bail!("last_user_protected");
        }
        Ok(self.users.remove(idx))
    }
}

/// Persistent marker that the online bootstrap flow has completed.
/// Stored alongside the user collection in the user-state file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapSentinel {
    pub completed_at: u64,
    pub by_username: String,
}

/// Persisted shape on disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct UserStateDocument {
    #[serde(default = "default_state_file_version")]
    version: u32,
    #[serde(default)]
    users: Vec<UserRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bootstrap_completed: Option<BootstrapSentinel>,
}

impl Default for UserStateDocument {
    fn default() -> Self {
        Self {
            version: STATE_FILE_VERSION,
            users: Vec::new(),
            bootstrap_completed: None,
        }
    }
}

fn default_state_file_version() -> u32 {
    STATE_FILE_VERSION
}

/// Normalise a username: trimmed, lowercase. Usernames are compared
/// case-insensitively because users routinely confuse `Alice` and
/// `alice` between admin set-up and login.
pub fn normalise_username(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Generate a new opaque user id. 16 random bytes hex-encoded, so it
/// looks UUID-shaped without pulling in the `uuid` crate.
pub fn new_user_id() -> String {
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

/// Load the user collection from the backend-managed user-state file.
/// Returns the default (empty) collection when no file exists yet; the
/// caller treats that as "bootstrap required".
pub fn load(path: &Path) -> Result<UserCollection> {
    let doc = load_state_document(path)?;
    Ok(UserCollection {
        version: COLLECTION_VERSION,
        users: doc.users,
    })
}

/// Persist the user collection while preserving the bootstrap
/// sentinel already present in the user-state file.
pub fn save(path: &Path, collection: &UserCollection) -> Result<()> {
    anyhow::ensure!(
        collection.version == COLLECTION_VERSION,
        "user collection has version {}, expected {}",
        collection.version,
        COLLECTION_VERSION
    );
    let mut doc = load_state_document(path)?;
    doc.version = STATE_FILE_VERSION;
    doc.users = collection.users.clone();
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

/// Persist the bootstrap-completed sentinel while preserving the user
/// collection already present in the user-state file.
pub fn mark_bootstrap_completed(path: &Path, sentinel: &BootstrapSentinel) -> Result<()> {
    let mut doc = load_state_document(path)?;
    doc.bootstrap_completed = Some(sentinel.clone());
    save_state_document(path, &doc)
}

fn load_state_document(path: &Path) -> Result<UserStateDocument> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(UserStateDocument::default());
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let doc: UserStateDocument = serde_json::from_str(&text)
        .with_context(|| format!("parsing user-state file {}", path.display()))?;
    anyhow::ensure!(
        doc.version == STATE_FILE_VERSION,
        "unsupported user-state file version {}",
        doc.version,
    );
    Ok(doc)
}

fn save_state_document(path: &Path, doc: &UserStateDocument) -> Result<()> {
    anyhow::ensure!(
        doc.version == STATE_FILE_VERSION,
        "user-state file has version {}, expected {}",
        doc.version,
        STATE_FILE_VERSION
    );
    let json = serde_json::to_vec_pretty(doc).context("serialising user-state file")?;
    let dir = path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "user-state file path '{}' has no parent directory",
            path.display()
        )
    })?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating user-state dir {}", dir.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("creating temp user-state file under {}", dir.display()))?;
    use std::io::Write;
    tmp.write_all(&json)
        .with_context(|| format!("writing temp user-state file under {}", dir.display()))?;
    tmp.flush()
        .with_context(|| format!("flushing temp user-state file under {}", dir.display()))?;
    tmp.persist(path).map_err(|e| {
        anyhow::anyhow!(
            "atomically persisting user-state file to {}: {e}",
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record(username: &str) -> UserRecord {
        UserRecord {
            id: new_user_id(),
            username: username.to_string(),
            password_hash:
                "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
                    .to_string(),
            created_at: 1000,
            password_changed_at: 1000,
        }
    }

    fn test_state_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("underwrap-user-state-test-{}.json", new_user_id()))
    }

    #[test]
    fn collection_round_trips_through_serde() {
        let mut c = UserCollection::default();
        c.add(sample_record("alice")).unwrap();
        c.add(sample_record("bob")).unwrap();
        let bytes = serde_json::to_vec(&c).unwrap();
        let parsed: UserCollection = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn add_rejects_duplicate_username_case_insensitively() {
        let mut c = UserCollection::default();
        c.add(sample_record("Alice")).unwrap();
        let err = c.add(sample_record("alice")).unwrap_err();
        assert!(err.to_string().contains("already exists"));
        assert_eq!(c.users[0].username, "alice");
    }

    #[test]
    fn add_rejects_empty_username() {
        let mut c = UserCollection::default();
        let err = c.add(sample_record("   ")).unwrap_err();
        assert!(err.to_string().contains("non-empty"));
    }

    #[test]
    fn add_rejects_non_argon2_hash() {
        let mut c = UserCollection::default();
        let mut bad = sample_record("alice");
        bad.password_hash = "plaintext".to_string();
        let err = c.add(bad).unwrap_err();
        assert!(err.to_string().contains("argon2"));
    }

    #[test]
    fn find_by_username_is_case_insensitive() {
        let mut c = UserCollection::default();
        c.add(sample_record("Alice")).unwrap();
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
        let mut c = UserCollection::default();
        c.add(sample_record("alice")).unwrap();
        save(&path, &c).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded, c);
    }

    #[test]
    fn load_rejects_unknown_state_file_version() {
        let path = test_state_path();
        std::fs::write(&path, r#"{"version":99,"users":[]}"#).unwrap();
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
        let mut c = UserCollection::default();
        c.add(sample_record("alice")).unwrap();
        save(&path, &c).unwrap();
        assert_eq!(load(&path).unwrap(), c);
        assert_eq!(load_bootstrap_sentinel(&path).unwrap().unwrap(), sentinel);
    }

    #[test]
    fn remove_refuses_last_user() {
        let mut c = UserCollection::default();
        c.add(sample_record("alice")).unwrap();
        let id = c.users[0].id.clone();
        let err = c.remove(&id).unwrap_err();
        assert_eq!(err.to_string(), "last_user_protected");
        assert_eq!(c.len(), 1, "the only user must still be present");
    }

    #[test]
    fn remove_succeeds_when_more_than_one_user_exists() {
        let mut c = UserCollection::default();
        c.add(sample_record("alice")).unwrap();
        c.add(sample_record("bob")).unwrap();
        let bob_id = c.users[1].id.clone();
        let removed = c.remove(&bob_id).unwrap();
        assert_eq!(removed.username, "bob");
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn debug_redacts_password_hash() {
        let r = sample_record("alice");
        let dbg = format!("{r:?}");
        assert!(
            !dbg.contains("argon2"),
            "Debug must not leak the PHC string"
        );
        assert!(dbg.contains("<redacted>"));
        let mut c = UserCollection::default();
        c.add(r).unwrap();
        let dbg = format!("{c:?}");
        assert!(!dbg.contains("argon2"));
    }

    #[test]
    fn load_rejects_v1_state_file() {
        let path = test_state_path();
        std::fs::write(&path, r#"{"version":1,"operators":[]}"#).unwrap();
        let err = load(&path).unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported user-state file version")
        );
    }
}
