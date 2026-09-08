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

    /// Create a new plaintext root exclusively with exact private mode.
    pub fn create(dir: &Path) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(dir)
                .with_context(|| format!("creating plain storage dir {}", dir.display()))?;
            if let Err(error) =
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            {
                let _ = std::fs::remove_dir(dir);
                return Err(error)
                    .with_context(|| format!("securing plain storage dir {}", dir.display()));
            }
        }
        #[cfg(not(unix))]
        std::fs::create_dir(dir)
            .with_context(|| format!("creating plain storage dir {}", dir.display()))?;

        match Self::open_existing(dir) {
            Ok(backend) => Ok(backend),
            Err(error) => {
                let _ = std::fs::remove_dir(dir);
                Err(error)
            }
        }
    }

    /// Open a pre-existing regular non-symlink plaintext root without creating it.
    pub fn open_existing(dir: &Path) -> Result<Self> {
        let metadata = std::fs::symlink_metadata(dir)
            .with_context(|| format!("inspecting plain storage dir {}", dir.display()))?;
        anyhow::ensure!(
            metadata.file_type().is_dir() && !metadata.file_type().is_symlink(),
            "plain storage path is not a directory: {}",
            dir.display(),
        );
        Ok(Self {
            dir: dir
                .canonicalize()
                .with_context(|| format!("canonicalizing plain storage dir {}", dir.display()))?,
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

    fn reopen_at(&self, dir: &Path) -> Result<Box<dyn StorageBackend>> {
        Ok(Box::new(Self::open_existing(dir)?))
    }
}

#[cfg(test)]
mod tests;
