//! Atomic on-mutation persistence of the libsecret section to a
//! tmpfs file in the namespace.
//!
//! The state file is the source of truth for the namespace parent
//! (`underwrap-ns`) after the namespace dies. Because PID 1 (the
//! target) exits and the kernel SIGKILLs every other namespace
//! member immediately, secretsd never gets a chance to respond to
//! a teardown signal — every mutation must be persisted before the
//! ack returns to the D-Bus caller.
//!
//! Writes are atomic: write to a sibling `.new` file, then rename
//! over the canonical path. A torn write on the canonical path
//! would mean the next `ns` read parses garbage and the run's
//! changes are lost.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use underwrap_secretsd_store::SecretStore;

use crate::section_store::section_from_store;

/// Project `store` back into a `LibsecretSection`, canonicalize, and
/// write atomically to `path`. The file is created with mode `0o600`
/// (the tmpfs is already mode `0o700` and inside the ns's mount
/// namespace, so this is defense in depth).
pub fn dump(store: &SecretStore, path: &Path) -> Result<()> {
    let section = section_from_store(store);
    let bytes = section
        .to_canonical_bytes()
        .context("canonicalizing libsecret section")?;
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
