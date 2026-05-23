//! User records: the people who can log in to the admin UI.
//!
//! On-disk layout is one file per user at `<users_dir>/<id>.json`. The
//! id is the file stem, so it is not serialized into the JSON body —
//! the loader backfills it from the filename. A separate
//! `<bootstrap_file>` (typically a sibling of `users_dir`) holds the
//! persistent bootstrap-completed sentinel; its mere presence signals
//! that the online bootstrap flow has completed.
//!
//! Authorisation is intentionally flat: any authenticated user can
//! perform any admin action. The "last user" guard prevents the
//! collection from going empty (which would re-trigger the bootstrap
//! flow).
//!
//! Password hashes are Argon2id PHC strings. We never log them; the
//! [`UserRecord`] `Debug` impl redacts them and tests assert that
//! tracing-captured CRUD output never contains the hash material.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Typed errors produced by mutating operations on [`UserCollection`].
///
/// Handlers match on the variant to pick the HTTP status and structured
/// `code` returned to admin clients; the rendered message stays as
/// fallback text for the response body.
#[derive(Debug, Error)]
pub enum UserMutationError {
    #[error("user '{0}' not found")]
    NotFound(String),
    #[error("user '{0}' already exists")]
    DuplicateUsername(String),
    #[error("cannot delete the last remaining user")]
    LastUserProtected,
    #[error("username must not be empty")]
    InvalidUsername,
    #[error("password_hash must be a PHC-encoded argon2 hash")]
    BadHash,
}

/// Default location of the backend-managed users directory.
pub const DEFAULT_USERS_DIR: &str = "/var/lib/underwrap-backend/users";

/// Default location of the persistent bootstrap-completed sentinel
/// file. A sibling of [`DEFAULT_USERS_DIR`].
pub const DEFAULT_BOOTSTRAP_FILE: &str = "/var/lib/underwrap-backend/bootstrap.json";

/// One user account.
///
/// `password_hash` is an Argon2id PHC string. It must never appear in
/// logs; the custom `Debug` impl below redacts it.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserRecord {
    /// Stable identifier. Random 16-byte hex string, generated server-side.
    ///
    /// On disk this is the file stem (`<id>.json`), so it is not
    /// serialized into the JSON body — the loader backfills it from
    /// the filename and [`save_user`] writes the body without it.
    #[serde(default, skip)]
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
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserCollection {
    pub users: Vec<UserRecord>,
}

impl std::fmt::Debug for UserCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `UserRecord`'s Debug impl already redacts the hash; we
        // implement Debug here to keep the same redaction-by-default
        // posture for the wrapper.
        f.debug_struct("UserCollection")
            .field("users", &self.users)
            .finish()
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
    pub fn add(&mut self, mut record: UserRecord) -> Result<(), UserMutationError> {
        record.username = normalise_username(&record.username);
        if record.username.is_empty() {
            return Err(UserMutationError::InvalidUsername);
        }
        if !record.password_hash.starts_with("$argon2") {
            return Err(UserMutationError::BadHash);
        }
        if self.find_by_username(&record.username).is_some() {
            return Err(UserMutationError::DuplicateUsername(record.username));
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
    pub fn update<F>(&mut self, id: &str, f: F) -> Result<&UserRecord, UserMutationError>
    where
        F: FnOnce(&mut UserRecord),
    {
        let idx = self
            .users
            .iter()
            .position(|u| u.id == id)
            .ok_or_else(|| UserMutationError::NotFound(id.to_string()))?;
        let mut next = self.users[idx].clone();
        f(&mut next);
        next.username = normalise_username(&next.username);
        if next.username.is_empty() {
            return Err(UserMutationError::InvalidUsername);
        }
        if !next.password_hash.starts_with("$argon2") {
            return Err(UserMutationError::BadHash);
        }
        // Reject username conflicts with any *other* user.
        if let Some(other) = self.find_by_username(&next.username)
            && other.id != next.id
        {
            return Err(UserMutationError::DuplicateUsername(next.username));
        }
        self.users[idx] = next;
        Ok(&self.users[idx])
    }

    /// Remove the user with id `id`. Refuses to remove the last user.
    pub fn remove(&mut self, id: &str) -> Result<UserRecord, UserMutationError> {
        let idx = self
            .users
            .iter()
            .position(|u| u.id == id)
            .ok_or_else(|| UserMutationError::NotFound(id.to_string()))?;
        if self.users.len() <= 1 {
            return Err(UserMutationError::LastUserProtected);
        }
        Ok(self.users.remove(idx))
    }
}

/// Persistent marker that the online bootstrap flow has completed.
/// Stored on disk in a dedicated sentinel file; the mere presence of
/// that file is the signal that bootstrap has run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapSentinel {
    pub completed_at: u64,
    pub by_username: String,
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

/// Path of the JSON file holding the record for user id `user_id`.
fn record_path(dir: &Path, user_id: &str) -> PathBuf {
    dir.join(format!("{user_id}.json"))
}

/// Parse a single `<id>.json` file. The id is taken from the file
/// stem and backfilled into the returned record; the body itself
/// does not carry it.
fn load_one(path: &Path) -> Result<UserRecord> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading user record {}", path.display()))?;
    let mut record: UserRecord = serde_json::from_str(&text)
        .with_context(|| format!("parsing user record {}", path.display()))?;
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "user record path '{}' has no valid file stem",
                path.display()
            )
        })?
        .to_string();
    anyhow::ensure!(
        is_valid_user_id(&id),
        "invalid user id '{}' derived from file {}",
        id,
        path.display()
    );
    record.id = id;
    Ok(record)
}

/// Load every user record found under `dir`. A missing directory is
/// treated as an empty collection; that is the state the bootstrap
/// flow expects to find on a fresh backend. Subdirectories and
/// non-`.json` siblings (e.g. `<id>.json.bak`) are ignored so
/// operator backups don't break the load. Hard-errors on duplicate
/// usernames across files — the in-memory collection enforces that
/// invariant, so two files claiming the same name indicates manual
/// tampering that should not start the daemon.
pub fn load(dir: &Path) -> Result<UserCollection> {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(UserCollection::default());
        }
        Err(e) => {
            return Err(e).with_context(|| format!("reading users directory {}", dir.display()));
        }
    };

    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in read {
        let entry =
            entry.with_context(|| format!("iterating users directory {}", dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("statting {}", entry.path().display()))?;
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        paths.push(path);
    }
    paths.sort();

    let mut users = Vec::new();
    let mut usernames = BTreeSet::new();
    for path in paths {
        let record = load_one(&path)?;
        anyhow::ensure!(
            !record.username.trim().is_empty(),
            "user '{}' has an empty username in {}",
            record.id,
            path.display()
        );
        anyhow::ensure!(
            record.password_hash.starts_with("$argon2"),
            "user '{}' has a non-argon2 password hash in {}",
            record.id,
            path.display()
        );
        anyhow::ensure!(
            usernames.insert(record.username.clone()),
            "duplicate username '{}' across files in {}",
            record.username,
            dir.display(),
        );
        users.push(record);
    }
    Ok(UserCollection { users })
}

/// Persist one user record to `<dir>/<id>.json` via the usual
/// tempfile + atomic rename. The on-disk body does not carry the id
/// — it is implied by the filename.
pub fn save_user(dir: &Path, record: &UserRecord) -> Result<()> {
    anyhow::ensure!(
        is_valid_user_id(&record.id),
        "invalid user id '{}'",
        record.id,
    );
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating users directory {}", dir.display()))?;
    let json = serde_json::to_vec_pretty(record).context("serialising user record")?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("creating temp user record under {}", dir.display()))?;
    use std::io::Write;
    tmp.write_all(&json).context("writing temp user record")?;
    tmp.as_file().sync_all().context("fsync temp user record")?;
    let target = record_path(dir, &record.id);
    tmp.persist(&target).map_err(|e| {
        anyhow::anyhow!(
            "atomically persisting user record to {}: {e}",
            target.display(),
        )
    })?;
    Ok(())
}

/// Remove `<dir>/<id>.json`. Missing files are tolerated so the
/// admin delete path remains idempotent under retry.
pub fn delete_user(dir: &Path, user_id: &str) -> Result<()> {
    let path = record_path(dir, user_id);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing user record {}", path.display())),
    }
}

/// True iff the bootstrap-completed sentinel file is present.
pub fn bootstrap_completed(bootstrap_file: &Path) -> Result<bool> {
    match std::fs::metadata(bootstrap_file) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("statting {}", bootstrap_file.display())),
    }
}

/// Read the sentinel record, if present.
pub fn load_bootstrap_sentinel(bootstrap_file: &Path) -> Result<Option<BootstrapSentinel>> {
    let text = match std::fs::read_to_string(bootstrap_file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(e).with_context(|| format!("reading {}", bootstrap_file.display()));
        }
    };
    let sentinel: BootstrapSentinel = serde_json::from_str(&text)
        .with_context(|| format!("parsing bootstrap sentinel {}", bootstrap_file.display()))?;
    Ok(Some(sentinel))
}

/// Persist the bootstrap-completed sentinel to its dedicated file.
/// Atomic via tempfile + rename so a crash partway through never
/// leaves a half-written sentinel that would lock further bootstrap
/// attempts behind unreadable JSON.
pub fn mark_bootstrap_completed(bootstrap_file: &Path, sentinel: &BootstrapSentinel) -> Result<()> {
    let json = serde_json::to_vec_pretty(sentinel).context("serialising bootstrap sentinel")?;
    let dir = bootstrap_file.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "bootstrap sentinel path '{}' has no parent directory",
            bootstrap_file.display()
        )
    })?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating bootstrap sentinel parent dir {}", dir.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("creating temp bootstrap sentinel under {}", dir.display()))?;
    use std::io::Write;
    tmp.write_all(&json)
        .context("writing temp bootstrap sentinel")?;
    tmp.as_file()
        .sync_all()
        .context("fsync temp bootstrap sentinel")?;
    tmp.persist(bootstrap_file).map_err(|e| {
        anyhow::anyhow!(
            "atomically persisting bootstrap sentinel to {}: {e}",
            bootstrap_file.display(),
        )
    })?;
    Ok(())
}

fn is_valid_user_id(id: &str) -> bool {
    crate::ids::is_valid_simple_id(id)
}

#[cfg(test)]
mod tests;
