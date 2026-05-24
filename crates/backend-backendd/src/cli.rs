//! CLI-side helpers used by the `underwrap-backendd` binary. Pulled
//! into the library crate so the per-function unit tests live in a
//! sibling `tests.rs` (binaries can't host a sibling-file submodule
//! directly).

use std::path::Path;

use anyhow::{Context, Result};

use crate::{BackendConfig, time, users};

pub fn parse_backend_config(config_text: &str) -> Result<BackendConfig> {
    serde_yaml::from_str(config_text).context("parsing config")
}

/// Add a user to the on-disk collection. Refuses if a user with the
/// same username already exists. Doesn't touch the
/// bootstrap-completed sentinel — recovery is intentionally not a
/// substitute for the online bootstrap (which is what locks the
/// endpoint going forward).
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
