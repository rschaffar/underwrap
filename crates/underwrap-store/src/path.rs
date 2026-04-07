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

    // Always canonicalize the parent directory and verify containment.
    // This closes a TOCTOU gap where a symlink could be planted in a
    // parent component between validation and the actual write/rename.
    if let Some(parent) = candidate.parent()
        && parent.exists()
    {
        let canonical_parent = parent
            .canonicalize()
            .with_context(|| format!("canonicalizing parent for {secret_id:?}"))?;
        if !canonical_parent.starts_with(base) {
            bail!(
                "secret parent escapes storage directory: {secret_id:?} -> {}",
                canonical_parent.display()
            );
        }
    }

    // For existing files, also verify the file itself.
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
///
/// Uses `tempfile::NamedTempFile` for guaranteed-unique temp files with
/// secure default permissions, then `persist()` for the atomic rename.
pub fn atomic_write(path: &Path, base: &Path, _secret_id: &str, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let parent = path.parent().unwrap_or(base);

    let mut tmp =
        tempfile::NamedTempFile::new_in(parent).context("creating temp file for atomic write")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o600))?;
    }

    std::io::Write::write_all(&mut tmp, data).context("writing secret to temp file")?;

    tmp.persist(path)
        .context("persisting temp file to final path")?;

    Ok(())
}

/// Create and canonicalize a storage directory.
pub fn prepare_dir(dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating storage dir {}", dir.display()))?;
    dir.canonicalize()
        .with_context(|| format!("canonicalizing storage dir {}", dir.display()))
}

/// List all files under `base/prefix` recursively, returning paths
/// relative to `base`. Only returns files (not directories).
/// If the prefix directory does not exist, returns an empty list.
pub fn list_files_recursive(base: &Path, prefix: &str) -> Result<Vec<String>> {
    let search_dir = if prefix.is_empty() {
        base.to_path_buf()
    } else {
        // Validate prefix components.
        for component in prefix.split('/') {
            if component.is_empty() || component == "." || component == ".." {
                bail!("invalid prefix: {prefix:?}");
            }
        }
        base.join(prefix)
    };

    if !search_dir.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    collect_files(base, &search_dir, &mut results)?;
    results.sort();
    Ok(results)
}

fn collect_files(base: &Path, dir: &Path, results: &mut Vec<String>) -> Result<()> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("listing directory {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let ft = entry.file_type()?;
        if ft.is_dir() {
            collect_files(base, &entry.path(), results)?;
        } else if ft.is_file()
            && let Ok(relative) = entry.path().strip_prefix(base)
            && let Some(s) = relative.to_str()
        {
            results.push(s.to_string());
        }
    }
    Ok(())
}

/// Remove empty parent directories up to (but not including) `base`.
pub fn cleanup_empty_parents(path: &Path, base: &Path) {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == base {
            break;
        }
        // Try to remove — fails silently if not empty.
        if std::fs::remove_dir(dir).is_err() {
            break;
        }
        current = dir.parent();
    }
}
