//! Secret storage backends.
//!
//! Phase 1: plain directory backend — one file per secret, owned by daemon user.

use anyhow::{Context, Result};
use std::path::PathBuf;
use underwrap_protocol::config::StorageConfig;

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn open(config: &StorageConfig) -> Result<Self> {
        match config {
            StorageConfig::Plain { dir } => {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("creating storage dir {}", dir.display()))?;
                Ok(Self { dir: dir.clone() })
            }
        }
    }

    /// Read secret material by id.  The id uses `/` separators which map to
    /// subdirectories (e.g. "github/token" -> "<dir>/github/token").
    pub fn read(&self, secret_id: &str) -> Result<Option<String>> {
        let path = self.secret_path(secret_id);
        match std::fs::read_to_string(&path) {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading secret {secret_id}")),
        }
    }

    /// Write (create or replace) secret material.
    pub fn write(&self, secret_id: &str, material: &str) -> Result<()> {
        let path = self.secret_path(secret_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, material)
            .with_context(|| format!("writing secret {secret_id}"))?;
        Ok(())
    }

    fn secret_path(&self, secret_id: &str) -> PathBuf {
        // Sanitise: reject path traversal attempts.
        assert!(
            !secret_id.contains(".."),
            "secret id must not contain '..'"
        );
        self.dir.join(secret_id)
    }
}
