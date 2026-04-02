//! Path validation shared across file-based backends.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// Validate a secret_id and resolve it to a safe filesystem path under `base`.
///
/// Rejects absolute paths, traversal (`.`, `..`), empty segments, and null bytes.
/// For existing files, also verifies the canonical path stays inside `base`.
pub fn validated_path(base: &Path, secret_id: &str) -> Result<PathBuf> {
    if secret_id.contains('\0') {
        bail!("secret id must not contain null bytes: {secret_id:?}");
    }
    if secret_id.is_empty() {
        bail!("secret id must not be empty");
    }
    if secret_id.starts_with('/') {
        bail!("secret id must not be an absolute path: {secret_id:?}");
    }
    for component in secret_id.split('/') {
        if component.is_empty() {
            bail!("secret id must not contain empty path segments: {secret_id:?}");
        }
        if component == "." || component == ".." {
            bail!("secret id must not contain '.' or '..': {secret_id:?}");
        }
    }

    let candidate = base.join(secret_id);

    if candidate.exists() {
        let canonical = candidate
            .canonicalize()
            .with_context(|| format!("canonicalizing secret path for {secret_id:?}"))?;
        if !canonical.starts_with(base) {
            bail!(
                "secret path escapes storage directory: {secret_id:?} -> {}",
                canonical.display()
            );
        }
    }

    Ok(candidate)
}

/// Atomically write `data` to `path` via temp-file + rename.
/// Sets file permissions to 0600 on unix.
pub fn atomic_write(path: &Path, base: &Path, secret_id: &str, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let parent = path.parent().unwrap_or(base);
    let tmp_path = parent.join(format!(
        ".tmp.{}.{}",
        std::process::id(),
        secret_id.replace('/', "_")
    ));

    std::fs::write(&tmp_path, data)
        .with_context(|| format!("writing temp file for secret {secret_id}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))?;
    }

    std::fs::rename(&tmp_path, path).with_context(|| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("renaming temp file for secret {secret_id}")
    })?;

    Ok(())
}

/// Create and canonicalize a storage directory.
pub fn prepare_dir(dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating storage dir {}", dir.display()))?;
    dir.canonicalize()
        .with_context(|| format!("canonicalizing storage dir {}", dir.display()))
}
