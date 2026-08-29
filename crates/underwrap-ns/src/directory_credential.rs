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

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource_blob::ResourceBlob;

/// The pure layout for one files-slot mount: where its staging tmpfs
/// subdirectory lives and where it binds into the target's home. Computed by
/// the parent with **no filesystem side effects** (see
/// [`plan_directory_projection`]) so the same layout drives both the child-side
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

#[derive(Debug)]
struct PlannedFile {
    key: String,
    mount_index: usize,
    relative_path: String,
}

/// Complete, side-effect-free semantic plan for one files projection. Secret
/// bytes remain solely in the decoded [`ResourceBlob`]; this plan retains only
/// validated keys, mount indexes, and relative destinations.
#[derive(Debug)]
pub struct DirectoryProjectionPlan {
    blob: ResourceBlob,
    mounts: Vec<DirectoryMountPlan>,
    files: Vec<PlannedFile>,
}

impl DirectoryProjectionPlan {
    pub fn blob(&self) -> &ResourceBlob {
        &self.blob
    }

    pub fn mounts(&self) -> &[DirectoryMountPlan] {
        &self.mounts
    }

    pub fn is_empty(&self) -> bool {
        self.mounts.is_empty()
    }
}

/// Compute the complete files projection with **no filesystem side effects**.
/// Every mount and key is clean, every key routes to exactly one strict mount
/// prefix, and no file key is an ancestor of another file key. Realization only
/// accepts this completed plan and therefore cannot discover those semantic
/// errors after staging writes begin.
pub fn plan_directory_projection(
    target_paths: &[String],
    staging_root: &Path,
    resource_id: &str,
    blob: ResourceBlob,
) -> Result<DirectoryProjectionPlan, String> {
    if target_paths.is_empty() {
        return Ok(DirectoryProjectionPlan {
            blob,
            mounts: Vec::new(),
            files: Vec::new(),
        });
    }

    let mut mounts = Vec::with_capacity(target_paths.len());
    for (index, mount_path) in target_paths.iter().enumerate() {
        validate_relative_path(mount_path)?;
        mounts.push(DirectoryMountPlan {
            mount_path: mount_path.clone(),
            staging_path: staging_root.join(format!("mount-{index}")),
        });
    }

    let keys: BTreeSet<&str> = blob.files.keys().map(String::as_str).collect();
    for key in &keys {
        validate_relative_path(key)?;
        let mut prefix = String::new();
        let components: Vec<&str> = key.split('/').collect();
        for component in &components[..components.len() - 1] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            if keys.contains(prefix.as_str()) {
                return Err(format!(
                    "resource '{resource_id}' has colliding file keys '{prefix}' and '{key}'; a regular file cannot also be a parent directory"
                ));
            }
        }
    }

    let mut files = Vec::with_capacity(blob.files.len());
    for key in blob.files.keys() {
        let mount_index = route_key_to_mount(key, target_paths, resource_id)?;
        files.push(PlannedFile {
            key: key.clone(),
            mount_index,
            relative_path: strip_mount_prefix(key, &mounts[mount_index].mount_path).to_string(),
        });
    }

    Ok(DirectoryProjectionPlan {
        blob,
        mounts,
        files,
    })
}

/// Realize a completed files projection. The staging tmpfs **root** must already
/// be mounted by the child after switching to uid X. Environmental filesystem
/// and mount operations can still fail, but routing, equality, cleanliness, and
/// aggregate file-shape semantics were settled before this function is called.
pub fn realize_directory_mounts(
    target_home: &Path,
    plan: &DirectoryProjectionPlan,
) -> Result<(), String> {
    // Phase 1: create per-mount staging directories.
    for mount in &plan.mounts {
        create_dir_mode(&mount.staging_path, 0o700)?;
    }

    // Phase 2: materialize only pre-routed files. The plan and blob originate
    // from the same immutable Checkout decode in `run`, so indexing cannot
    // introduce a new semantic failure during realization.
    for file in &plan.files {
        let content = &plan.blob.files[&file.key];
        materialize_file(
            &plan.mounts[file.mount_index].staging_path,
            &file.relative_path,
            content.as_slice(),
        )?;
    }

    // Phase 3: bind-mount each staging dir at its target path.
    for mount in &plan.mounts {
        let mountpoint = ensure_mount_target(target_home, &mount.mount_path)?;
        bind_mount_dir(&mount.staging_path, &mountpoint)?;
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

/// Locate the single mount whose target path is a strict path-prefix of `key`.
/// Equality, no route, and multiple routes are all semantic planning errors.
fn route_key_to_mount(
    key: &str,
    target_paths: &[String],
    resource_id: &str,
) -> Result<usize, String> {
    if let Some(mount) = target_paths.iter().find(|mount| key == mount.as_str()) {
        return Err(format!(
            "resource '{resource_id}' has file key '{key}' that equals mount path '{mount}'; a file cannot share its name with a directory mountpoint"
        ));
    }

    let matches: Vec<usize> = target_paths
        .iter()
        .enumerate()
        .filter_map(|(index, mount)| is_path_prefix(mount, key).then_some(index))
        .collect();
    match matches.as_slice() {
        [index] => Ok(*index),
        [] => Err(format!(
            "resource '{resource_id}' has file key '{key}' but no configured mount path is a prefix of it; either add the missing mount to files.paths or remove the file from the backend resource"
        )),
        _ => Err(format!(
            "resource '{resource_id}' has file key '{key}' that routes to multiple configured mount paths; files.paths must provide exactly one strict route"
        )),
    }
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
    // SAFETY: `source` and `target` are live NUL-terminated path buffers;
    // MS_BIND permits null filesystem-type and data pointers, and the trusted
    // bind flag plus syscall return are checked before the mount is used.
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
