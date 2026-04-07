//! Plain file-based storage backend.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::StorageBackend;
use crate::path::{
    atomic_write, cleanup_empty_parents, list_files_recursive, prepare_dir, validated_path,
};

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

    #[test]
    fn list_empty() {
        let (_dir, b) = test_backend();
        let ids = b.list("").unwrap();
        assert!(ids.is_empty());
    }

    #[test]
    fn list_with_prefix() {
        let (_dir, b) = test_backend();
        b.write("a/b/item1", "v1").unwrap();
        b.write("a/b/item2", "v2").unwrap();
        b.write("a/c/item3", "v3").unwrap();
        b.write("d/item4", "v4").unwrap();

        let mut ids = b.list("a/b").unwrap();
        ids.sort();
        assert_eq!(ids, vec!["a/b/item1", "a/b/item2"]);

        let mut ids = b.list("a").unwrap();
        ids.sort();
        assert_eq!(ids, vec!["a/b/item1", "a/b/item2", "a/c/item3"]);

        let mut ids = b.list("").unwrap();
        ids.sort();
        assert_eq!(ids, vec!["a/b/item1", "a/b/item2", "a/c/item3", "d/item4"]);
    }

    #[test]
    fn list_nonexistent_prefix() {
        let (_dir, b) = test_backend();
        let ids = b.list("nonexistent").unwrap();
        assert!(ids.is_empty());
    }

    #[test]
    fn delete_existing() {
        let (_dir, b) = test_backend();
        b.write("tok", "val").unwrap();
        assert!(b.delete("tok").unwrap());
        assert!(b.read("tok").unwrap().is_none());
    }

    #[test]
    fn delete_nonexistent() {
        let (_dir, b) = test_backend();
        assert!(!b.delete("nope").unwrap());
    }

    #[test]
    fn delete_cleans_empty_parents() {
        let (dir, b) = test_backend();
        b.write("a/b/c/tok", "val").unwrap();
        assert!(b.delete("a/b/c/tok").unwrap());
        // Empty parent dirs should be cleaned up.
        assert!(!dir.path().join("a/b/c").exists());
        assert!(!dir.path().join("a/b").exists());
        assert!(!dir.path().join("a").exists());
    }

    #[test]
    fn delete_preserves_sibling_entries() {
        let (_dir, b) = test_backend();
        b.write("a/b/item1", "v1").unwrap();
        b.write("a/b/item2", "v2").unwrap();
        assert!(b.delete("a/b/item1").unwrap());
        // item2 should still be there.
        assert_eq!(b.read("a/b/item2").unwrap().unwrap(), "v2");
        // And listable.
        let ids = b.list("a/b").unwrap();
        assert_eq!(ids, vec!["a/b/item2"]);
    }
}
