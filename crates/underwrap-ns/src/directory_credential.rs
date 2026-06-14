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

/// The pure layout for one files-slot mount: where its staging tmpfs
/// subdirectory lives and where it binds into the target's home. Computed by
/// the parent with **no filesystem side effects** (see
/// [`plan_directory_mounts`]) so the same layout drives both the child-side
/// realize step (at fsuid X) and the parent-side end-of-run writeback walk
/// (under a `setfsuid(X)` window).
#[derive(Debug, Clone)]
pub struct DirectoryMountPlan {
    /// Wrapper-configured relative path (e.g. `.config/gh`). Key prefix when
    /// reconstructing blob file keys at writeback; also where (under
    /// `<target_home>`) the staging binds.
    pub mount_path: String,
    /// Host-side staging tmpfs subdirectory (`<staging_root>/mount-{i}`)
    /// materialized into and bound at the mount path; walked at writeback.
    pub staging_path: PathBuf,
}

/// Compute the deterministic files-slot layout — one [`DirectoryMountPlan`]
/// per configured mount path — with **no filesystem side effects**. The parent
/// always computes this (so it can drive the writeback walk); the realize step
/// ([`realize_directory_mounts`]) consumes it to actually create and bind the
/// staging.
///
/// `staging_path` for mount `i` is `<staging_root>/mount-{i}`. Mount paths are
/// validated clean/relative here so a malformed config fails before any mount
/// is made; the realize step joins them under `<target_home>`.
pub fn plan_directory_mounts(
    target_paths: &[String],
    staging_root: &Path,
) -> Result<Vec<DirectoryMountPlan>, String> {
    if target_paths.is_empty() {
        return Err("files-slot binding has no target paths".to_string());
    }
    let mut plans = Vec::with_capacity(target_paths.len());
    for (index, mount_path) in target_paths.iter().enumerate() {
        validate_relative_path(mount_path)?;
        plans.push(DirectoryMountPlan {
            mount_path: mount_path.clone(),
            staging_path: staging_root.join(format!("mount-{index}")),
        });
    }
    Ok(plans)
}

/// Realize the planned files-slot mounts: create each mount's tmpfs staging
/// subdirectory, materialize the blob files routed to it, ensure the mountpoint
/// exists, and bind-mount the staging over it. The staging tmpfs **root** must
/// already be mounted by the child after switching to uid X (see
/// [`crate::mounts::setup_directory_staging_tmpfs`]); this only populates it.
///
/// Errors when a blob file key has no matching mount path or equals a mount
/// path exactly: a file there would collide with a directory mountpoint, and
/// silently dropping content the backend handed us would hide misconfiguration.
pub fn realize_directory_mounts(
    resource_id: &str,
    blob: &ResourceBlob,
    target_home: &Path,
    plans: &[DirectoryMountPlan],
) -> Result<(), String> {
    // Phase 1: create per-mount staging directories.
    for plan in plans {
        create_dir_mode(&plan.staging_path, 0o700)?;
    }

    // Phase 2: route every blob file key to its mount and write it
    // into that mount's staging dir.
    let mount_paths: Vec<String> = plans.iter().map(|p| p.mount_path.clone()).collect();
    for (key, content) in &blob.files {
        validate_relative_path(key)?;
        let mount_index = route_key_to_mount(key, &mount_paths, resource_id)?;
        let relative = strip_mount_prefix(key, &plans[mount_index].mount_path);
        materialize_file(
            &plans[mount_index].staging_path,
            relative,
            content.as_slice(),
        )?;
    }

    // Phase 3: bind-mount each staging dir at its target path.
    for plan in plans {
        let mountpoint = ensure_mount_target(target_home, &plan.mount_path)?;
        bind_mount_dir(&plan.staging_path, &mountpoint)?;
    }

    Ok(())
}

fn create_dir_mode(path: &Path, mode: u32) -> Result<(), String> {
    std::fs::create_dir_all(path)
        .map_err(|e| format!("creating staging directory {}: {e}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("setting permissions on {}: {e}", path.display()))
}

/// Walk every mount's staging tmpfs and produce a `files` section
/// suitable for inclusion in the end-of-run Checkin blob. Each
/// relative path is re-prefixed with the mount's `mount_path` so the
/// emitted keys match the shape the backend originally handed us.
///
/// The walk rejects symlinks (a symlink in the staging aborts the writeback) and
/// reads only regular files. For an isolated target the supervisor performs this
/// walk inside a `setfsuid(X)` window (see [`crate::run_parent`]), which bounds
/// every read to the target's own uid X — so a target-planted symlink could only
/// redirect to content X already controls, never to the caller's files. That
/// uid bound, not a fancier resolver, is what keeps the cross-uid writeback safe.
pub fn collect_staging_files(
    plans: &[DirectoryMountPlan],
) -> Result<BTreeMap<String, SecretBytes>, String> {
    let mut out = BTreeMap::new();
    for plan in plans {
        collect_files_with_prefix(
            &plan.staging_path,
            &plan.staging_path,
            &plan.mount_path,
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

fn ensure_mount_target(target_home: &Path, relative_path: &str) -> Result<PathBuf, String> {
    validate_relative_path(relative_path)?;
    let mut current = target_home.to_path_buf();
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
            }
            Err(e) => {
                return Err(format!("stat {}: {e}", next.display()));
            }
        }
        current = next;
    }

    Ok(current)
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
