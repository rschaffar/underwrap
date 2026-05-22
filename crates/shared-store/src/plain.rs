//! Plain file-based storage backend.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::StorageBackend;
use crate::path::{
    atomic_write, cleanup_empty_parents, list_files_recursive, prepare_dir, validated_path,
};
use underwrap_protocol::message::SecretBytes;

/// Plaintext files in a directory, owned by the daemon user.
pub struct PlainBackend {
    dir: PathBuf,
}

impl PlainBackend {
    pub fn open(dir: &Path) -> Result<Self> {
        Ok(Self {
            dir: prepare_dir(dir)?,
        })
    }
}

impl StorageBackend for PlainBackend {
    fn read(&self, secret_id: &str) -> Result<Option<SecretBytes>> {
        let path = validated_path(&self.dir, secret_id)?;
        match std::fs::read(&path) {
            Ok(data) => Ok(Some(data.into())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading secret {secret_id}")),
        }
    }

    fn write(&self, secret_id: &str, material: &[u8]) -> Result<()> {
        let path = validated_path(&self.dir, secret_id)?;
        atomic_write(&path, &self.dir, secret_id, material)
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        list_files_recursive(&self.dir, prefix)
    }

    fn delete(&self, secret_id: &str) -> Result<bool> {
        let path = validated_path(&self.dir, secret_id)?;
        match std::fs::remove_file(&path) {
            Ok(()) => {
                // Clean up empty parent directories up to base.
                cleanup_empty_parents(&path, &self.dir);
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e).with_context(|| format!("deleting secret {secret_id}")),
        }
    }
}

#[cfg(test)]
mod tests;
