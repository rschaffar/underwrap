//! Plain file-based storage backend.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::StorageBackend;
use crate::path::{atomic_write, prepare_dir, validated_path};

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
    fn read(&self, secret_id: &str) -> Result<Option<String>> {
        let path = validated_path(&self.dir, secret_id)?;
        match std::fs::read_to_string(&path) {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading secret {secret_id}")),
        }
    }

    fn write(&self, secret_id: &str, material: &str) -> Result<()> {
        let path = validated_path(&self.dir, secret_id)?;
        atomic_write(&path, &self.dir, secret_id, material.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_backend() -> (tempfile::TempDir, PlainBackend) {
        let dir = tempfile::TempDir::new().unwrap();
        let backend = PlainBackend::open(dir.path()).unwrap();
        (dir, backend)
    }

    #[test]
    fn read_nonexistent_returns_none() {
        let (_dir, b) = test_backend();
        assert!(b.read("nope").unwrap().is_none());
    }

    #[test]
    fn write_then_read() {
        let (_dir, b) = test_backend();
        b.write("my/secret", "hello").unwrap();
        assert_eq!(b.read("my/secret").unwrap().unwrap(), "hello");
    }

    #[test]
    fn overwrite() {
        let (_dir, b) = test_backend();
        b.write("tok", "v1").unwrap();
        b.write("tok", "v2").unwrap();
        assert_eq!(b.read("tok").unwrap().unwrap(), "v2");
    }

    #[test]
    fn deep_hierarchy() {
        let (_dir, b) = test_backend();
        b.write("a/b/c/d/e", "deep").unwrap();
        assert_eq!(b.read("a/b/c/d/e").unwrap().unwrap(), "deep");
    }

    #[test]
    fn rejects_dotdot() {
        let (_dir, b) = test_backend();
        assert!(b.write("../etc/passwd", "pwned").is_err());
        assert!(b.read("../etc/passwd").is_err());
    }

    #[test]
    fn rejects_absolute_path() {
        let (_dir, b) = test_backend();
        assert!(b.write("/etc/shadow", "pwned").is_err());
    }

    #[test]
    fn rejects_dot_component() {
        let (_dir, b) = test_backend();
        assert!(b.write("./secret", "data").is_err());
    }

    #[test]
    fn rejects_empty_segments() {
        let (_dir, b) = test_backend();
        assert!(b.write("a//b", "data").is_err());
    }

    #[test]
    fn rejects_empty_id() {
        let (_dir, b) = test_backend();
        assert!(b.write("", "data").is_err());
    }

    #[test]
    fn rejects_null_bytes() {
        let (_dir, b) = test_backend();
        assert!(b.write("secret\0id", "data").is_err());
    }

    #[test]
    fn empty_material() {
        let (_dir, b) = test_backend();
        b.write("empty", "").unwrap();
        assert_eq!(b.read("empty").unwrap().unwrap(), "");
    }

    #[test]
    fn atomic_write_leaves_no_temp_files() {
        let (dir, b) = test_backend();
        b.write("tok", "val").unwrap();
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let name = entry.unwrap().file_name().to_string_lossy().to_string();
            assert!(!name.starts_with(".tmp."), "temp file left behind: {name}");
        }
    }
}
