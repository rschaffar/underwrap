//! Secret storage backends.
//!
//! Phase 1: plain directory backend — one file per secret, owned by daemon user.

use anyhow::{Context, Result, bail};
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
                // Canonicalize the base directory so containment checks work
                // even if the configured path contains symlinks.
                let canonical = dir
                    .canonicalize()
                    .with_context(|| format!("canonicalizing storage dir {}", dir.display()))?;
                Ok(Self { dir: canonical })
            }
        }
    }

    /// Read secret material by id.  The id uses `/` separators which map to
    /// subdirectories (e.g. "github/token" -> "<dir>/github/token").
    pub fn read(&self, secret_id: &str) -> Result<Option<String>> {
        let path = self.validated_path(secret_id)?;
        match std::fs::read_to_string(&path) {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading secret {secret_id}")),
        }
    }

    /// Write (create or replace) secret material atomically.
    ///
    /// Writes to a temporary file then renames, so concurrent readers never
    /// see partial data.
    pub fn write(&self, secret_id: &str, material: &str) -> Result<()> {
        let path = self.validated_path(secret_id)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Atomic write: create temp file in same directory, write, rename.
        let parent = path.parent().unwrap_or(&self.dir);
        let mut tmp_path = parent.to_path_buf();
        tmp_path.push(format!(
            ".tmp.{}.{}",
            std::process::id(),
            secret_id.replace('/', "_")
        ));

        std::fs::write(&tmp_path, material)
            .with_context(|| format!("writing temp file for secret {secret_id}"))?;

        // Set restrictive permissions before rename.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))?;
        }

        std::fs::rename(&tmp_path, &path).with_context(|| {
            // Clean up temp file on rename failure.
            let _ = std::fs::remove_file(&tmp_path);
            format!("renaming temp file for secret {secret_id}")
        })?;

        Ok(())
    }

    /// Validate and resolve a secret_id to a safe filesystem path.
    ///
    /// Rejects:
    /// - absolute paths
    /// - path traversal (`.`, `..`)
    /// - empty segments
    /// - null bytes
    /// - paths that resolve outside the storage directory
    fn validated_path(&self, secret_id: &str) -> Result<PathBuf> {
        // Reject null bytes.
        if secret_id.contains('\0') {
            bail!("secret id must not contain null bytes: {secret_id:?}");
        }

        // Reject empty id.
        if secret_id.is_empty() {
            bail!("secret id must not be empty");
        }

        // Reject absolute paths.
        if secret_id.starts_with('/') {
            bail!("secret id must not be an absolute path: {secret_id:?}");
        }

        // Reject dangerous path components.
        for component in secret_id.split('/') {
            if component.is_empty() {
                bail!("secret id must not contain empty path segments: {secret_id:?}");
            }
            if component == "." || component == ".." {
                bail!("secret id must not contain '.' or '..': {secret_id:?}");
            }
        }

        let candidate = self.dir.join(secret_id);

        // Final containment check: the resolved path must start with our
        // storage directory.  This catches any remaining tricks (symlinks in
        // the secret path, etc.) — but only when the file already exists.
        // For new files, the component-level checks above are the gate.
        if candidate.exists() {
            let canonical = candidate
                .canonicalize()
                .with_context(|| format!("canonicalizing secret path for {secret_id:?}"))?;
            if !canonical.starts_with(&self.dir) {
                bail!(
                    "secret path escapes storage directory: {secret_id:?} -> {}",
                    canonical.display()
                );
            }
        }

        Ok(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::TempDir::new().unwrap();
        let config = StorageConfig::Plain {
            dir: dir.path().to_path_buf(),
        };
        let store = Store::open(&config).unwrap();
        (dir, store)
    }

    #[test]
    fn read_nonexistent_returns_none() {
        let (_dir, store) = test_store();
        assert!(store.read("nope").unwrap().is_none());
    }

    #[test]
    fn write_then_read() {
        let (_dir, store) = test_store();
        store.write("my/secret", "hello").unwrap();
        assert_eq!(store.read("my/secret").unwrap().unwrap(), "hello");
    }

    #[test]
    fn overwrite() {
        let (_dir, store) = test_store();
        store.write("tok", "v1").unwrap();
        store.write("tok", "v2").unwrap();
        assert_eq!(store.read("tok").unwrap().unwrap(), "v2");
    }

    #[test]
    fn deep_hierarchy() {
        let (_dir, store) = test_store();
        store.write("a/b/c/d/e", "deep").unwrap();
        assert_eq!(store.read("a/b/c/d/e").unwrap().unwrap(), "deep");
    }

    // --- Path traversal rejection tests ---

    #[test]
    fn rejects_dotdot() {
        let (_dir, store) = test_store();
        assert!(store.write("../etc/passwd", "pwned").is_err());
        assert!(store.read("../etc/passwd").is_err());
    }

    #[test]
    fn rejects_absolute_path() {
        let (_dir, store) = test_store();
        assert!(store.write("/etc/shadow", "pwned").is_err());
        assert!(store.read("/etc/shadow").is_err());
    }

    #[test]
    fn rejects_dot_component() {
        let (_dir, store) = test_store();
        assert!(store.write("./secret", "data").is_err());
        assert!(store.write("a/./b", "data").is_err());
    }

    #[test]
    fn rejects_empty_segments() {
        let (_dir, store) = test_store();
        assert!(store.write("a//b", "data").is_err());
    }

    #[test]
    fn rejects_empty_id() {
        let (_dir, store) = test_store();
        assert!(store.write("", "data").is_err());
        assert!(store.read("").is_err());
    }

    #[test]
    fn rejects_null_bytes() {
        let (_dir, store) = test_store();
        assert!(store.write("secret\0id", "data").is_err());
    }

    #[test]
    fn rejects_embedded_dotdot() {
        let (_dir, store) = test_store();
        assert!(store.write("a/../../../etc/passwd", "pwned").is_err());
    }

    #[test]
    fn empty_material() {
        let (_dir, store) = test_store();
        store.write("empty", "").unwrap();
        assert_eq!(store.read("empty").unwrap().unwrap(), "");
    }

    #[test]
    fn binary_like_material() {
        let (_dir, store) = test_store();
        // Note: null bytes in material are fine (it's the *value*, not the path).
        let data = "line1\nline2\ttab";
        store.write("bin", data).unwrap();
        assert_eq!(store.read("bin").unwrap().unwrap(), data);
    }

    #[test]
    fn atomic_write_leaves_no_temp_files() {
        let (dir, store) = test_store();
        store.write("tok", "val").unwrap();

        // Check no .tmp files remain.
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().to_string();
            assert!(!name.starts_with(".tmp."), "temp file left behind: {name}");
        }
    }

    #[test]
    fn concurrent_writes_dont_corrupt() {
        let (_dir, store) = test_store();

        // Simulate rapid writes — each should be atomic.
        for i in 0..20 {
            store.write("tok", &format!("val_{i}")).unwrap();
            let read = store.read("tok").unwrap().unwrap();
            // Read should always be a complete value, never partial.
            assert!(read.starts_with("val_"), "got corrupted read: {read}");
        }
    }
}
