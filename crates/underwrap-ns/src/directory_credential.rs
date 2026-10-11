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
use std::io::Read;
use std::path::Path;

use crate::directory_handle::Directory;

use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource_blob::ResourceBlob;

/// Pure, pathname-free staging plan, computed before any filesystem effects.
#[derive(Debug, Clone)]
pub struct DirectoryMountPlan {
    pub mount_path: String,
}

/// Actual projection authority, prepared under target filesystem ids before
/// fork. Both supervisors inherit the same pinned directory objects; neither
/// stores or re-resolves an absolute staging pathname.
#[derive(Debug)]
pub struct DirectoryMount {
    pub mount_path: String,
    pub directory: Directory,
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
    for mount_path in target_paths {
        validate_relative_path(mount_path)?;
        mounts.push(DirectoryMountPlan {
            mount_path: mount_path.clone(),
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

/// Create only empty directories on one private tmpfs. The caller runs this
/// in its private mount namespace and checked target-fsid window after pure
/// planning. No brokered bytes are materialized until the relay becomes X.
pub fn prepare_directory_mounts(
    plan: &DirectoryProjectionPlan,
    runtime_dir: &Path,
    isolation: crate::target_uid::Isolation,
) -> Result<Vec<DirectoryMount>, String> {
    if plan.is_empty() {
        return Ok(Vec::new());
    }
    let runtime = Directory::open_target_home(runtime_dir)?;
    runtime.require_identity_mode(isolation.uid, isolation.gid, 0o700)?;
    let attachment = runtime.target_child(".underwrap-files", 0)?;
    attachment.require_identity_mode(isolation.uid, isolation.gid, 0)?;
    let mount = crate::mounts::create_directory_staging_tmpfs()?;
    // Attach while the fsmount descriptor still owns the anonymous mount.
    // open_tree can then clone its directories inside this namespace; no later
    // operation resolves the attachment's pathname.
    let root = Directory::from_mount(mount, &attachment)?;
    root.require_identity_mode(isolation.uid, isolation.gid, 0o700)?;
    prepare_mount_directories(plan, &root)
}

fn prepare_mount_directories(
    plan: &DirectoryProjectionPlan,
    root: &Directory,
) -> Result<Vec<DirectoryMount>, String> {
    plan.mounts
        .iter()
        .enumerate()
        .map(|(index, mount)| {
            Ok(DirectoryMount {
                mount_path: mount.mount_path.clone(),
                directory: root.create_child(&format!("mount-{index}"), 0o700)?,
            })
        })
        .collect()
}

/// Materialize and bind the completed projection after the relay becomes X.
pub fn realize_directory_mounts(
    target_home: &Path,
    plan: &DirectoryProjectionPlan,
    mounts: &[DirectoryMount],
) -> Result<(), String> {
    materialize_directory_files(plan, mounts)?;
    for mount in mounts {
        let mountpoint = ensure_mount_target(target_home, &mount.mount_path)?;
        mount.directory.bind_to(&mountpoint)?;
    }
    Ok(())
}

fn materialize_directory_files(
    plan: &DirectoryProjectionPlan,
    mounts: &[DirectoryMount],
) -> Result<(), String> {
    if mounts.len() != plan.mounts.len()
        || mounts
            .iter()
            .zip(&plan.mounts)
            .any(|(mount, planned)| mount.mount_path != planned.mount_path)
    {
        return Err("projection handles do not match the completed plan".into());
    }
    for file in &plan.files {
        materialize_file(
            &mounts[file.mount_index].directory,
            &file.relative_path,
            plan.blob.files[&file.key].as_slice(),
        )?;
    }
    Ok(())
}

/// Walk the originally pinned projection objects under the target-fsid window.
/// Each component is opened with NOFOLLOW and each read uses the resulting fd;
/// neither ancestor renames nor replacement staging names redirect collection.
/// The target and its descendants have been reaped before this walk begins.
pub fn collect_staging_files(
    mounts: &[DirectoryMount],
) -> Result<BTreeMap<String, SecretBytes>, String> {
    let mut out = BTreeMap::new();
    for mount in mounts {
        collect_files_with_prefix(&mount.directory, "", &mount.mount_path, &mut out)?;
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
    directory: &Directory,
    prefix: &str,
    mount_prefix: &str,
    out: &mut BTreeMap<String, SecretBytes>,
) -> Result<(), String> {
    for name in directory.entries()? {
        let name = name
            .to_str()
            .ok_or_else(|| "projection entry name is not valid UTF-8".to_string())?;
        let relative = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        validate_relative_path(&relative)?;
        let mut file = directory.open_entry(name)?;
        let metadata = file
            .metadata()
            .map_err(|e| format!("reading projection metadata: {e}"))?;
        if metadata.is_dir() {
            collect_files_with_prefix(&Directory::from_file(file), &relative, mount_prefix, out)?;
        } else if metadata.is_file() {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .map_err(|e| format!("reading projection file: {e}"))?;
            out.insert(
                format!("{mount_prefix}/{relative}"),
                SecretBytes::from(bytes),
            );
        } else {
            return Err("projection directory contains unsupported entry type".into());
        }
    }
    Ok(())
}

fn materialize_file(
    directory: &Directory,
    relative_path: &str,
    content: &[u8],
) -> Result<(), String> {
    validate_relative_path(relative_path)?;
    let mut components = relative_path.split('/').peekable();
    let mut parents = Vec::new();
    let mut current = directory;
    while let Some(component) = components.next() {
        if components.peek().is_none() {
            return current.create_file(component, content);
        }
        let child = match current.open_child(component) {
            Ok(child) => child,
            Err(_) => current.create_child(component, 0o700)?,
        };
        parents.push(child);
        current = parents.last().unwrap();
    }
    Err("empty projection file path".into())
}

fn ensure_mount_target(target_home: &Path, relative_path: &str) -> Result<Directory, String> {
    validate_relative_path(relative_path)?;
    let mut current = Directory::open_target_home(target_home)?;
    let components: Vec<&str> = relative_path.split('/').collect();
    for (index, component) in components.iter().enumerate() {
        current = current.target_child(
            component,
            if index + 1 == components.len() {
                0
            } else {
                0o700
            },
        )?;
    }
    Ok(current)
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

#[cfg(test)]
mod tests;
