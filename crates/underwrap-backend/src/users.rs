//! User records: the people who can log in to the admin UI.
//!
//! On-disk layout is one file per user at `<users_dir>/<id>.json`. The
//! id is the file stem, so it is not serialized into the JSON body —
//! the loader backfills it from the filename. A separate
//! `<bootstrap_file>` (typically a sibling of `users_dir`) holds the
//! persistent bootstrap-completed sentinel; its presence signals that the
//! offline first-operator command completed both persistence steps.
//!
//! Authorisation is intentionally flat: any authenticated user can
//! perform any admin action. The "last user" guard prevents the
//! operator collection from becoming empty.
//!
//! Password hashes are Argon2id PHC strings. We never log them; the
//! [`UserRecord`] `Debug` impl redacts them and tests assert that
//! tracing-captured CRUD output never contains the hash material.

use std::path::Path;

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

/// The in-memory user collection. `Debug` is derived through
/// [`UserRecord`]'s manual `Debug`, which redacts the password hash —
/// no separate impl needed here.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserCollection {
    pub users: Vec<UserRecord>,
}

impl UserCollection {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.users.len()
    }

    /// True when no user records exist. Used by read-only setup status and
    /// offline first-operator freshness checks.
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

impl crate::storage::json_dir_store::DirRecord for UserRecord {
    const KIND: &'static str = "user";
    const KIND_PLURAL_DIR: &'static str = "users directory";

    fn id(&self) -> &str {
        &self.id
    }

    fn set_id(&mut self, id: String) {
        self.id = id;
    }

    fn validate_fields(&self, path: &Path) -> Result<()> {
        anyhow::ensure!(
            !self.username.trim().is_empty(),
            "user '{}' has an empty username in {}",
            self.id,
            path.display(),
        );
        anyhow::ensure!(
            self.password_hash.starts_with("$argon2"),
            "user '{}' has a non-argon2 password hash in {}",
            self.id,
            path.display(),
        );
        Ok(())
    }

    fn uniqueness_keys(&self) -> Vec<(&'static str, String)> {
        vec![("username", self.username.clone())]
    }
}

/// Load every user record found under `dir`. A missing directory is
/// treated as an empty collection; that is the state the bootstrap
/// flow expects to find on a fresh backend.
pub fn load(dir: &Path) -> Result<UserCollection> {
    Ok(UserCollection {
        users: crate::storage::json_dir_store::load_dir(dir)?,
    })
}

/// Persist one user record to `<dir>/<id>.json` via the usual
/// tempfile + atomic rename.
pub fn save_user(dir: &Path, record: &UserRecord) -> Result<()> {
    crate::storage::json_dir_store::save(dir, record)
}

/// Remove `<dir>/<id>.json`. Missing files are tolerated so the
/// admin delete path remains idempotent under retry.
pub fn delete_user(dir: &Path, user_id: &str) -> Result<()> {
    crate::storage::json_dir_store::delete(dir, "user", user_id)
}

/// True iff any directory entry occupies the bootstrap-sentinel path.
///
/// The final path component is deliberately not followed: a dangling symlink
/// is existing, non-fresh authentication state and must fail closed. An
/// ordinary missing final entry retains the fresh-backend meaning.
pub fn bootstrap_completed(bootstrap_file: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(bootstrap_file) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("statting {}", bootstrap_file.display())),
    }
}

/// Mark the bootstrap-completed sentinel as set. Only the file's
/// presence carries information; the body is intentionally empty so
/// there is no metadata to mis-parse on a later read. Atomic via
/// tempfile + rename so a crash partway through never leaves an
/// inconsistent half-state.
pub fn mark_bootstrap_completed(bootstrap_file: &Path) -> Result<()> {
    let dir = bootstrap_file.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "bootstrap sentinel path '{}' has no parent directory",
            bootstrap_file.display()
        )
    })?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating bootstrap sentinel parent dir {}", dir.display()))?;
    let tmp = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("creating temp bootstrap sentinel under {}", dir.display()))?;
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

#[cfg(test)]
mod tests;
