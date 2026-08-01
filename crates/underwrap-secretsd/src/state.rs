//! Atomic on-mutation persistence of the libsecret section to a
//! tmpfs file in the namespace.
//!
//! The state file is the source of truth for host `underwrap-ns` after it
//! cleanly reaps the trusted namespace PID-1 relay. The configured target is
//! PID 2; the relay reaps it, frames its status, then exits so the kernel
//! SIGKILLs any remaining helpers. Secretsd has no graceful teardown handshake,
//! so every mutation must be persisted before the ack returns to the D-Bus
//! caller.
//!
//! Writes are atomic: write to a sibling `.new` file, then rename
//! over the canonical path. A torn write on the canonical path
//! would mean the next `ns` read parses garbage and the run's
//! changes are lost.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};

use underwrap_secretsd_store::SecretStore;

use crate::section_store::section_from_store;

/// Default upper bound on the persisted libsecret state, in bytes. The
/// staging tmpfs `underwrap-ns` mounts for the bus dir is `size=4m`; this cap
/// is well under that and far larger than any real libsecret keyring needs.
/// It exists so a compromised target cannot grow the keyring without bound
/// (filling the tmpfs / churning IO) via repeated D-Bus mutations.
pub const DEFAULT_MAX_STATE_BYTES: u64 = 1024 * 1024;

static MAX_STATE_BYTES: OnceLock<u64> = OnceLock::new();

/// Set the persisted-state size cap (from the `--max-state-bytes` CLI arg).
/// Called once at startup before serving; ignored if called again.
pub fn configure_max_state_bytes(max: u64) {
    let _ = MAX_STATE_BYTES.set(max);
}

fn configured_max_state_bytes() -> u64 {
    MAX_STATE_BYTES
        .get()
        .copied()
        .unwrap_or(DEFAULT_MAX_STATE_BYTES)
}

/// Project `store` back into a `LibsecretSection`, canonicalize, and
/// write atomically to `path`. The file is created with mode `0o600`
/// (the tmpfs is already mode `0o700` and inside the ns's mount
/// namespace, so this is defense in depth).
pub fn dump(store: &SecretStore, path: &Path) -> Result<()> {
    dump_with_cap(store, path, configured_max_state_bytes())
}

fn dump_with_cap(store: &SecretStore, path: &Path, max_bytes: u64) -> Result<()> {
    let section = section_from_store(store);
    let bytes = section
        .to_canonical_bytes()
        .context("canonicalizing libsecret section")?;
    // Enforce the cap BEFORE touching the on-disk file: a rejected mutation
    // must leave the previously-persisted (valid) state intact rather than
    // truncating it.
    if bytes.len() as u64 > max_bytes {
        bail!(
            "libsecret keyring state is {} bytes, exceeding the {max_bytes}-byte cap; \
             refusing to persist (mutation rejected)",
            bytes.len()
        );
    }
    write_atomic(path, &bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("state path '{}' has no parent", path.display()))?;
    let tmp_path: PathBuf = parent.join(format!(
        ".{}.new",
        path.file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!(
                "state path '{}' has no usable file name",
                path.display()
            ))?
    ));

    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp_path)
            .with_context(|| format!("opening {} for atomic write", tmp_path.display()))?;
        f.write_all(bytes)
            .with_context(|| format!("writing {}", tmp_path.display()))?;
        f.sync_all()
            .with_context(|| format!("fsync {}", tmp_path.display()))?;
    }
    std::fs::rename(&tmp_path, path)
        .with_context(|| format!("renaming {} to {}", tmp_path.display(), path.display()))
}

/// Best-effort variant for D-Bus handler paths: logs at `error` and
/// returns the original `fdo::Error` on failure so the caller can map
/// it into a uniform `Failed(...)` D-Bus error.
pub fn dump_or_log(store: &SecretStore, path: &Path) -> Result<(), zbus::fdo::Error> {
    if let Err(e) = dump(store, path) {
        tracing::error!(error = ?e, path = %path.display(), "persisting libsecret state failed");
        return Err(zbus::fdo::Error::Failed(format!(
            "persisting libsecret state failed: {e:#}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
