//! Files-slot staging and bind-mount management for the wrapper's
//! one resource.
//!
//! When the resource has `files_target_paths`, ns gives each
//! configured mount its own tmpfs staging subdirectory and bind-mounts
//! it at the mount's target path. Every key in the blob's `files`
//! section must be a path under exactly one configured mount path —
//! the leading mount path is stripped and the file is materialized
//! inside that mount's staging dir. Blob keys that don't match any
//! configured mount path (or that equal a mount path exactly) abort
//! the wrapper. The staging tmpfs is always read-write to the target
//! — write authorization is enforced server-side at `Checkin` time,
//! not at the mount layer.
//!
//! At end of run, [`writeback`](crate::writeback) walks each mount's
//! staging dir and re-prefixes the relative paths with the mount's
//! target path to reconstruct the full keys for the Checkin blob.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource_blob::ResourceBlob;

pub struct DirectoryMount {
    pub target_mount_path: PathBuf,
    pub created_mount_dirs: Vec<PathBuf>,
    /// Staging tmpfs subdirectory backing this mount. Its contents are
    /// bind-mounted at `target_mount_path`.
    pub staging_path: PathBuf,
    /// The wrapper-config-declared relative mount path
    /// (e.g. `.config/gh`). Used to re-prefix file keys at writeback.
    pub mount_relative_path: String,
}

pub struct MountedDirectory {
    /// One entry per configured mount path, each with its own staging
    /// directory. The NixOS module rejects overlapping mount paths, so
    /// every blob file key matches at most one mount.
    pub mounts: Vec<DirectoryMount>,
}

/// Give each configured mount path its own tmpfs staging subdirectory
/// under `staging_root`, materialize blob files into the mount whose
/// path is a prefix of the file key, and bind-mount each staging
/// subdirectory at its `target_path` (relative to `target_home`). The
/// staging tmpfs is always read-write — the server gates persistence
/// via the `Checkin` access mode.
///
/// Errors when a blob file key has no matching mount path or equals a
/// mount path exactly: a file there would collide with a directory
/// mountpoint, and silently dropping content the backend handed us
/// would hide misconfiguration.
pub fn prepare_directory_mount(
    resource_id: &str,
    blob: &ResourceBlob,
    target_paths: &[String],
    target_home: &Path,
    staging_root: &Path,
) -> Result<MountedDirectory, String> {
    if target_paths.is_empty() {
        return Err(format!(
            "resource '{resource_id}' has no target paths but a files-slot binding"
        ));
    }
    for target_path in target_paths {
        validate_relative_path(target_path)?;
    }

    // Phase 1: create per-mount staging directories.
    create_dir_mode(staging_root, 0o700)?;
    let staging_paths: Vec<PathBuf> = (0..target_paths.len())
        .map(|mount_index| {
            let staging_path = staging_root.join(format!("mount-{mount_index}"));
            create_dir_mode(&staging_path, 0o700)?;
            Ok(staging_path)
        })
        .collect::<Result<_, String>>()?;

    // Phase 2: route every blob file key to its mount and write it
    // into that mount's staging dir.
    for (key, content) in &blob.files {
        validate_relative_path(key)?;
        let mount_index = route_key_to_mount(key, target_paths, resource_id)?;
        let relative = strip_mount_prefix(key, &target_paths[mount_index]);
        materialize_file(&staging_paths[mount_index], relative, content.as_slice())?;
    }

    // Phase 3: bind-mount each staging dir at its target path and
    // emit the final `DirectoryMount` records.
    let mut mounts: Vec<DirectoryMount> = Vec::with_capacity(target_paths.len());
    for (staging_path, mount_relative_path) in staging_paths.into_iter().zip(target_paths.iter()) {
        let (target_mount_path, created_mount_dirs) =
            ensure_mount_target(target_home, mount_relative_path)?;
        bind_mount_dir(&staging_path, &target_mount_path)?;
        mounts.push(DirectoryMount {
            target_mount_path,
            created_mount_dirs,
            staging_path,
            mount_relative_path: mount_relative_path.clone(),
        });
    }

    Ok(MountedDirectory { mounts })
}

fn create_dir_mode(path: &Path, mode: u32) -> Result<(), String> {
    std::fs::create_dir_all(path)
        .map_err(|e| format!("creating staging directory {}: {e}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("setting permissions on {}: {e}", path.display()))
}

/// Walk every mount's staging tmpfs and produce a `files` section
/// suitable for inclusion in the end-of-run Checkin blob. Each
/// relative path is re-prefixed with the mount's target path so the
/// emitted keys match the shape the backend originally handed us.
pub fn collect_staging_files(
    directory: &MountedDirectory,
) -> Result<BTreeMap<String, SecretBytes>, String> {
    let mut out = BTreeMap::new();
    for mount in &directory.mounts {
        collect_files_with_prefix(
            &mount.staging_path,
            &mount.staging_path,
            &mount.mount_relative_path,
            &mut out,
        )?;
    }
    Ok(out)
}

/// Locate the single mount whose target path is a strict path-prefix
/// of `key`. Errors when no mount matches or when the key equals a
/// mount path exactly. Mount paths are validated non-overlapping by
/// both the NixOS module and the daemon config loader, so at most one
/// mount can ever match — first match is also only match.
fn route_key_to_mount(
    key: &str,
    target_paths: &[String],
    resource_id: &str,
) -> Result<usize, String> {
    for (index, mount) in target_paths.iter().enumerate() {
        if key == mount.as_str() {
            return Err(format!(
                "resource '{resource_id}' has file key '{key}' that equals mount path '{mount}'; a file cannot share its name with a directory mountpoint"
            ));
        }
        if is_path_prefix(mount, key) {
            return Ok(index);
        }
    }
    Err(format!(
        "resource '{resource_id}' has file key '{key}' but no configured mount path is a prefix of it; either add the missing mount to files.paths or remove the file from the backend resource"
    ))
}

/// True iff `full` is `prefix` followed by `/` and at least one more
/// character — i.e. `prefix` is a strict path-component prefix of
/// `full`. Differs from `str::starts_with` in that `.config/gh` is
/// not a path-prefix of `.config/ghext/x` (the next byte must be
/// `/`, not just any continuation).
fn is_path_prefix(prefix: &str, full: &str) -> bool {
    full.len() > prefix.len() + 1
        && full.as_bytes()[prefix.len()] == b'/'
        && full.as_bytes()[..prefix.len()] == *prefix.as_bytes()
}

fn strip_mount_prefix<'a>(key: &'a str, mount: &str) -> &'a str {
    // Caller has already verified `is_path_prefix(mount, key)`, so the
    // `mount.len() + 1` byte boundary is at a UTF-8 boundary (`/`).
    &key[mount.len() + 1..]
}

pub fn cleanup_directory_mount(directory: &MountedDirectory) -> Result<(), String> {
    let mut first_error = None;
    for mount in directory.mounts.iter().rev() {
        if let Err(e) = unmount_path(&mount.target_mount_path) {
            first_error.get_or_insert(e);
            continue;
        }
        for dir in mount.created_mount_dirs.iter().rev() {
            if let Err(e) = std::fs::remove_dir(dir)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                first_error.get_or_insert(format!("removing mountpoint {}: {e}", dir.display()));
            }
        }
    }
    if let Some(err) = first_error {
        Err(err)
    } else {
        Ok(())
    }
}

/// Walk the tmpfs subtree rooted at `current` (under `root`) and
/// insert each regular file into `out` with key
/// `"{mount_prefix}/{path_relative_to_root}"`. Symlinks and other
/// non-regular entries abort the walk so they can never reach the
/// backend.
fn collect_files_with_prefix(
    root: &Path,
    current: &Path,
    mount_prefix: &str,
    out: &mut BTreeMap<String, SecretBytes>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(current)
        .map_err(|e| format!("listing directory {}: {e}", current.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("reading directory entry: {e}"))?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|e| format!("stat {}: {e}", path.display()))?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return Err(format!(
                "managed directory '{}' contains a symlink at {}",
                root.display(),
                path.display()
            ));
        }
        if file_type.is_dir() {
            collect_files_with_prefix(root, &path, mount_prefix, out)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|e| format!("stripping prefix from {}: {e}", path.display()))?
                .to_str()
                .ok_or_else(|| format!("path {} is not valid UTF-8", path.display()))?;
            validate_relative_path(relative)?;
            let bytes = std::fs::read(&path)
                .map_err(|e| format!("reading staged file {}: {e}", path.display()))?;
            out.insert(
                format!("{mount_prefix}/{relative}"),
                SecretBytes::from(bytes),
            );
        } else {
            return Err(format!(
                "managed directory '{}' contains unsupported entry type at {}",
                root.display(),
                path.display()
            ));
        }
    }
    Ok(())
}

fn materialize_file(
    staging_root: &Path,
    relative_path: &str,
    content: &[u8],
) -> Result<(), String> {
    let target = staging_root.join(relative_path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating staging directory {}: {e}", parent.display()))?;
        set_mode(parent, 0o700)?;
    }
    std::fs::write(&target, content)
        .map_err(|e| format!("writing staged file {}: {e}", target.display()))?;
    set_mode(&target, 0o600)?;
    Ok(())
}

fn ensure_mount_target(
    target_home: &Path,
    relative_path: &str,
) -> Result<(PathBuf, Vec<PathBuf>), String> {
    validate_relative_path(relative_path)?;
    let mut current = target_home.to_path_buf();
    let mut created = Vec::new();
    let components: Vec<&str> = relative_path.split('/').collect();

    for (index, component) in components.iter().enumerate() {
        let next = current.join(component);
        match std::fs::symlink_metadata(&next) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(format!(
                        "target path {} contains a symlink component",
                        next.display()
                    ));
                }
                if !metadata.is_dir() {
                    return Err(format!(
                        "target path {} exists but is not a directory",
                        next.display()
                    ));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&next)
                    .map_err(|err| format!("creating mountpoint {}: {err}", next.display()))?;
                let is_final = index + 1 == components.len();
                set_mode(&next, if is_final { 0o000 } else { 0o700 })?;
                created.push(next.clone());
            }
            Err(e) => {
                return Err(format!("stat {}: {e}", next.display()));
            }
        }
        current = next;
    }

    Ok((current, created))
}

fn bind_mount_dir(source: &Path, target: &Path) -> Result<(), String> {
    let source = path_to_cstring(source)?;
    let target = path_to_cstring(target)?;
    let ret = unsafe {
        libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND,
            std::ptr::null(),
        )
    };
    if ret != 0 {
        return Err(format!(
            "bind-mounting {} -> {} failed: {}",
            source.to_string_lossy(),
            target.to_string_lossy(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn unmount_path(path: &Path) -> Result<(), String> {
    let path = path_to_cstring(path)?;
    let ret = unsafe { libc::umount2(path.as_ptr(), libc::MNT_DETACH) };
    if ret != 0 {
        return Err(format!(
            "unmounting {} failed: {}",
            path.to_string_lossy(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("setting permissions on {}: {e}", path.display()))
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("path must not be empty".to_string());
    }
    if path.starts_with('/') {
        return Err("path must be relative".to_string());
    }
    if path.contains('\0') {
        return Err("path must not contain null bytes".to_string());
    }
    for component in path.split('/') {
        if component.is_empty() {
            return Err(format!("path '{path}' contains an empty segment"));
        }
        if component == "." || component == ".." {
            return Err(format!("path '{path}' must be clean"));
        }
    }
    Ok(())
}

fn path_to_cstring(path: &Path) -> Result<std::ffi::CString, String> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| format!("path '{}' contains a null byte", path.display()))
}

#[cfg(test)]
mod tests;
