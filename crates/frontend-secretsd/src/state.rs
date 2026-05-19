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
mod tests {
    use super::*;
    use std::collections::HashMap;
    use underwrap_protocol::resource_blob::LibsecretSection;
    use underwrap_secretsd_store::{Collection as StoreCollection, Item as StoreItem};

    fn populated_store() -> SecretStore {
        let mut store = SecretStore::new_default();
        store.collections.clear();
        store.aliases.clear();

        let mut items = HashMap::new();
        items.insert(
            "gh-cli".to_string(),
            StoreItem {
                secret: b"ghp_secret".to_vec(),
                content_type: "text/plain".to_string(),
                attributes: HashMap::from([("server".to_string(), "github.com".to_string())]),
                label: "gh CLI".to_string(),
                created: 100,
                modified: 200,
            },
        );
        store.collections.insert(
            "default".to_string(),
            StoreCollection {
                label: "Default".to_string(),
                items,
                created: 10,
                modified: 20,
            },
        );
        store
            .aliases
            .insert("default".to_string(), "default".to_string());
        store
    }

    #[test]
    fn dump_writes_canonical_bytes_atomically() {
        let tempdir = tempfile::TempDir::new().unwrap();
        let path = tempdir.path().join("libsecret-state.jcs");

        dump(&populated_store(), &path).unwrap();

        let read = std::fs::read(&path).unwrap();
        let section = LibsecretSection::from_canonical_bytes(&read).unwrap();
        assert!(section.collections.contains_key("default"));
    }

    #[test]
    fn dump_replaces_existing_file_atomically() {
        let tempdir = tempfile::TempDir::new().unwrap();
        let path = tempdir.path().join("libsecret-state.jcs");

        std::fs::write(&path, b"stale").unwrap();
        dump(&populated_store(), &path).unwrap();

        let read = std::fs::read(&path).unwrap();
        assert_ne!(read, b"stale");
        let _ = LibsecretSection::from_canonical_bytes(&read).unwrap();
    }
}
