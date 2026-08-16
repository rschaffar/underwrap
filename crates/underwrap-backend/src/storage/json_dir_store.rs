//! Generic on-disk record store: `<dir>/<id>.json` files with the id
//! implied by the filename. Three backend registries — users,
//! stores, and runtime client instances — share the same load/save/
//! delete shape. This module is their single source of truth.
//!
//! Each domain implements [`DirRecord`] to plug in its
//! per-entity validation, uniqueness keys, and the human-friendly
//! `KIND` string that goes into error messages.
//!
//! On-disk shape: pretty-printed JSON, one file per record. After the
//! temporary file is synced, `tempfile::NamedTempFile::persist` gives
//! atomic replacement visibility. The parent directory is not fsynced,
//! so this does not claim power-loss durability. Reads tolerate a missing
//! directory (fresh backend) and skip non-`.json` siblings so operator
//! backups like `<id>.json.bak` do not break the load.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub(crate) trait DirRecord: Serialize + DeserializeOwned + Sized {
    /// Friendly singular noun for error messages: "user", "store",
    /// "runtime client instance".
    const KIND: &'static str;
    /// Friendly plural for the directory: "users directory",
    /// "stores directory", "clients directory".
    const KIND_PLURAL_DIR: &'static str;

    fn id(&self) -> &str;
    fn set_id(&mut self, id: String);

    /// Per-record field validation called after deserialisation and
    /// id backfill. `path` is the file we read from so error
    /// messages can name it.
    fn validate_fields(&self, path: &Path) -> Result<()>;

    /// Uniqueness keys for cross-file collision detection. Each
    /// returned `(kind, value)` pair must be unique across every
    /// record in the directory; the generic loader hard-errors on
    /// the first collision. `kind` is a short label used in the
    /// error message ("username", "name", "credential fingerprint").
    fn uniqueness_keys(&self) -> Vec<(&'static str, String)>;
}

fn record_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

fn load_one<T: DirRecord>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {} record {}", T::KIND, path.display()))?;
    let mut record: T = serde_json::from_str(&text)
        .with_context(|| format!("parsing {} record {}", T::KIND, path.display()))?;
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{} record path '{}' has no valid file stem",
                T::KIND,
                path.display(),
            )
        })?
        .to_string();
    anyhow::ensure!(
        crate::ids::is_valid_id(&id),
        "invalid {} id '{}' derived from file {}",
        T::KIND,
        id,
        path.display(),
    );
    record.set_id(id);
    record.validate_fields(path)?;
    Ok(record)
}

/// Load every `<id>.json` record in `dir`. A missing directory is
/// treated as an empty registry — fresh backends boot from nothing.
/// Hard-errors on duplicate ids or any colliding uniqueness key, so
/// filesystem tampering (or a backup file renamed into place) can
/// not silently start the daemon in an inconsistent state.
pub(crate) fn load_dir<T: DirRecord>(dir: &Path) -> Result<Vec<T>> {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(e)
                .with_context(|| format!("reading {} {}", T::KIND_PLURAL_DIR, dir.display()));
        }
    };

    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in read {
        let entry =
            entry.with_context(|| format!("iterating {} {}", T::KIND_PLURAL_DIR, dir.display()))?;
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

    let mut records: Vec<T> = Vec::new();
    let mut ids: BTreeSet<String> = BTreeSet::new();
    let mut uniqueness: BTreeMap<&'static str, BTreeSet<String>> = BTreeMap::new();

    for path in paths {
        let record = load_one::<T>(&path)?;
        anyhow::ensure!(
            ids.insert(record.id().to_string()),
            "duplicate {} id '{}' across files in {}",
            T::KIND,
            record.id(),
            dir.display(),
        );
        for (kind, value) in record.uniqueness_keys() {
            let bucket = uniqueness.entry(kind).or_default();
            anyhow::ensure!(
                bucket.insert(value.clone()),
                "duplicate {} {} '{}' across files in {}",
                T::KIND,
                kind,
                value,
                dir.display(),
            );
        }
        records.push(record);
    }
    Ok(records)
}

/// Persist `record` to `<dir>/<id>.json` via the usual tempfile +
/// atomic rename. The on-disk body never carries the id — it is
/// implied by the filename and re-attached on load via [`DirRecord::set_id`].
pub(crate) fn save<T: DirRecord>(dir: &Path, record: &T) -> Result<()> {
    anyhow::ensure!(
        crate::ids::is_valid_id(record.id()),
        "invalid {} id '{}'",
        T::KIND,
        record.id(),
    );
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating {} {}", T::KIND_PLURAL_DIR, dir.display()))?;
    let json = serde_json::to_vec_pretty(record)
        .with_context(|| format!("serialising {} record", T::KIND))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("creating temp {} record under {}", T::KIND, dir.display()))?;
    use std::io::Write;
    tmp.write_all(&json)
        .with_context(|| format!("writing temp {} record", T::KIND))?;
    tmp.as_file()
        .sync_all()
        .with_context(|| format!("fsync temp {} record", T::KIND))?;
    let target = record_path(dir, record.id());
    tmp.persist(&target).map_err(|e| {
        anyhow::anyhow!(
            "atomically persisting {} record to {}: {e}",
            T::KIND,
            target.display(),
        )
    })?;
    Ok(())
}

/// Remove `<dir>/<id>.json`. Missing files are tolerated so the
/// admin delete path stays idempotent under retry.
pub(crate) fn delete(dir: &Path, kind: &'static str, id: &str) -> Result<()> {
    let path = record_path(dir, id);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing {kind} record {}", path.display())),
    }
}
