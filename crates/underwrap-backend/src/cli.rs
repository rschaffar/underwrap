//! CLI-side helpers used by the `underwrap-backend` binary. Pulled
//! into the library crate so the per-function unit tests live in a
//! sibling `tests.rs` (binaries can't host a sibling-file submodule
//! directly).

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::{BackendConfig, time, users};

pub fn parse_backend_config(config_text: &str) -> Result<BackendConfig> {
    serde_yaml::from_str(config_text).context("parsing config")
}

/// The held descriptor owns an advisory bootstrap lock on a stable inode.
///
/// The lock file is deliberately retained after use: unlinking it would let a
/// second process open a different inode and bypass an active lock. Closing the
/// descriptor, including automatic close at process exit, releases `flock` so a
/// crashed command cannot leave a stale lock.
struct BootstrapLock {
    _file: File,
}

fn bootstrap_lock_path(bootstrap_file: &Path) -> Result<PathBuf> {
    let parent = bootstrap_file.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "bootstrap-user: bootstrap sentinel path '{}' has no parent directory",
            bootstrap_file.display()
        )
    })?;
    let mut name = OsString::from(bootstrap_file.file_name().ok_or_else(|| {
        anyhow::anyhow!(
            "bootstrap-user: bootstrap sentinel path '{}' has no file name",
            bootstrap_file.display()
        )
    })?);
    name.push(".lock");
    Ok(parent.join(name))
}

fn acquire_bootstrap_lock(bootstrap_file: &Path) -> Result<BootstrapLock> {
    let path = bootstrap_lock_path(bootstrap_file)?;
    let parent = path.parent().expect("lock path has the sentinel parent");
    std::fs::create_dir_all(parent).with_context(|| {
        format!(
            "bootstrap-user: creating lock directory {}",
            parent.display()
        )
    })?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&path)
        .with_context(|| format!("bootstrap-user: opening lock file {}", path.display()))?;

    // SAFETY: `file.as_raw_fd()` is a live descriptor for the duration of the
    // call and `flock` takes no pointers. A successful `LOCK_EX` remains owned
    // by this open file description until `_file` is closed or the process
    // exits. Every error is checked and aborts before authentication state is
    // read or written.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("bootstrap-user: locking {}", path.display()));
    }
    Ok(BootstrapLock { _file: file })
}

/// Create the first operator through deployment-side filesystem authority.
///
/// Callers must quiesce the backend service and provide the password through
/// stdin. The stable cross-process lock covers the authoritative freshness
/// check, password hashing, user commit, and sentinel commit. The user is
/// intentionally committed first: if the sentinel write fails, the valid user
/// remains available for ordinary login and is never rolled back.
pub fn bootstrap_user(
    users_dir: &Path,
    bootstrap_file: &Path,
    username: &str,
    password: &str,
) -> Result<()> {
    bootstrap_user_with(
        users_dir,
        bootstrap_file,
        username,
        password,
        users::save_user,
        users::mark_bootstrap_completed,
    )
}

fn bootstrap_user_with<Save, Mark>(
    users_dir: &Path,
    bootstrap_file: &Path,
    username: &str,
    password: &str,
    save_user: Save,
    mark_completed: Mark,
) -> Result<()>
where
    Save: FnOnce(&Path, &users::UserRecord) -> Result<()>,
    Mark: FnOnce(&Path) -> Result<()>,
{
    let normalised = users::normalise_username(username);
    if normalised.is_empty() {
        anyhow::bail!("bootstrap-user: username must not be empty");
    }
    if let Err(code) = crate::auth::validate_password(password) {
        anyhow::bail!("bootstrap-user: password rejected ({code})");
    }

    let _lock = acquire_bootstrap_lock(bootstrap_file)?;

    if users::bootstrap_completed(bootstrap_file)
        .context("bootstrap-user: reading bootstrap sentinel")?
    {
        anyhow::bail!("bootstrap-user: bootstrap has already been completed");
    }
    let mut collection = users::load(users_dir).context("bootstrap-user: loading operators")?;
    if !collection.is_empty() {
        anyhow::bail!("bootstrap-user: an operator already exists");
    }

    let hash = crate::auth::hash_password(password).context("bootstrap-user: hashing password")?;
    let now = time::now_epoch_secs();
    let record = users::UserRecord {
        id: users::new_user_id(),
        username: normalised,
        password_hash: hash,
        created_at: now,
        password_changed_at: now,
    };
    collection
        .add(record.clone())
        .context("bootstrap-user: validating initial operator")?;

    save_user(users_dir, &record).context("bootstrap-user: persisting initial operator")?;
    mark_completed(bootstrap_file).with_context(|| {
        "bootstrap-user: initial operator was persisted, but the bootstrap sentinel could not be persisted; the operator remains available for login"
    })?;
    Ok(())
}

/// Add a user to the on-disk collection. Refuses if a user with the
/// same username already exists. Doesn't touch the bootstrap-completed
/// sentinel; recovery remains distinct from first-operator provisioning.
pub fn recover_user(users_dir: &Path, username: &str, password: &str) -> Result<()> {
    if password.is_empty() {
        anyhow::bail!("recover-user: password must not be empty");
    }
    let normalised = users::normalise_username(username);
    if normalised.is_empty() {
        anyhow::bail!("recover-user: username must not be empty");
    }
    let collection = users::load(users_dir)?;
    if collection.find_by_username(&normalised).is_some() {
        anyhow::bail!(
            "recover-user: user '{}' already exists; refusing to overwrite",
            normalised
        );
    }
    let hash = crate::auth::hash_password(password)?;
    let now = time::now_epoch_secs();
    let record = users::UserRecord {
        id: users::new_user_id(),
        username: normalised,
        password_hash: hash,
        created_at: now,
        password_changed_at: now,
    };
    users::save_user(users_dir, &record)?;
    Ok(())
}

#[cfg(test)]
mod tests;
