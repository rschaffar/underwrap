//! Files-slot staging and bind-mount management for one resource.
//!
//! For every resource with `files_target_paths`, ns materializes the
//! `files` section of the resource blob (from `Checkout`) into a
//! per-resource staging tmpfs directory, then bind-mounts that staging
//! directory at each target path. The staging tmpfs is always
//! read-write to the target — write authorization is enforced
//! server-side at `Checkin` time, not at the mount layer.
//!
//! At end of run, [`writeback`](crate::writeback) walks the tmpfs and
//! composes the new `files` section that becomes part of the
//! Checkin blob.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource_blob::ResourceBlob;

pub struct DirectoryMount {
    pub target_mount_path: PathBuf,
    pub created_mount_dirs: Vec<PathBuf>,
}

pub struct MountedDirectory {
    pub resource_id: String,
    /// All bind-mount targets backed by `staging_path`. A single backend
    /// resource can be exposed at multiple namespace-local paths; they
    /// all share the same staging directory.
    pub mounts: Vec<DirectoryMount>,
    pub staging_path: PathBuf,
}

/// One resource's binding plus its decoded blob, ready to be staged.
pub struct ResourceMountInput<'a> {
    pub resource_id: &'a str,
    pub blob: &'a ResourceBlob,
    pub target_paths: &'a [String],
}

/// Materialize the `files` section of each resource's blob into a
/// per-resource tmpfs staging directory under `staging_root`, then
/// bind-mount that staging dir at every `target_path` (relative to
/// `target_home`). The staging tmpfs is always read-write — the
/// server gates persistence via the `Checkin` access mode.
pub fn prepare_directory_mounts(
    inputs: &[ResourceMountInput<'_>],
    target_home: &Path,
    staging_root: &Path,
) -> Result<Vec<MountedDirectory>, String> {
    let mut mounted = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        if input.target_paths.is_empty() {
            return Err(format!(
                "resource '{}' has no target paths but a files-slot binding",
                input.resource_id
            ));
        }
        for target_path in input.target_paths {
            validate_relative_path(target_path)?;
        }

        let staging_path = staging_root.join(format!("dir-{index}"));
        std::fs::create_dir_all(&staging_path)
            .map_err(|e| format!("creating staging directory {}: {e}", staging_path.display()))?;
        std::fs::set_permissions(&staging_path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("setting permissions on {}: {e}", staging_path.display()))?;

        for (relative_path, content) in &input.blob.files {
            validate_relative_path(relative_path)?;
            materialize_file(&staging_path, relative_path, content.as_slice())?;
        }

        let mut mounts = Vec::with_capacity(input.target_paths.len());
        for target_path in input.target_paths {
            let (target_mount_path, created_mount_dirs) =
                ensure_mount_target(target_home, target_path)?;
            bind_mount_dir(&staging_path, &target_mount_path)?;
            mounts.push(DirectoryMount {
                target_mount_path,
                created_mount_dirs,
            });
        }

        mounted.push(MountedDirectory {
            resource_id: input.resource_id.to_string(),
            mounts,
            staging_path,
        });
    }
    Ok(mounted)
}

/// Walk the resource's staging tmpfs and produce a `files` section
/// suitable for inclusion in the end-of-run Checkin blob. Returns a
/// JCS-friendly `BTreeMap`.
pub fn collect_staging_files(
    directory: &MountedDirectory,
) -> Result<BTreeMap<String, SecretBytes>, String> {
    let mut out = BTreeMap::new();
    collect_files_recursive(&directory.staging_path, &directory.staging_path, &mut out)?;
    Ok(out)
}

pub fn cleanup_directory_mounts(directories: &[MountedDirectory]) -> Result<(), String> {
    let mut first_error = None;
    for directory in directories.iter().rev() {
        for mount in directory.mounts.iter().rev() {
            if let Err(e) = unmount_path(&mount.target_mount_path) {
                first_error.get_or_insert(e);
                continue;
            }
            for dir in mount.created_mount_dirs.iter().rev() {
                if let Err(e) = std::fs::remove_dir(dir)
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    first_error
                        .get_or_insert(format!("removing mountpoint {}: {e}", dir.display()));
                }
            }
        }
    }
    if let Some(err) = first_error {
        Err(err)
    } else {
        Ok(())
    }
}

fn collect_files_recursive(
    root: &Path,
    current: &Path,
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
            collect_files_recursive(root, &path, out)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|e| format!("stripping prefix from {}: {e}", path.display()))?
                .to_str()
                .ok_or_else(|| format!("path {} is not valid UTF-8", path.display()))?
                .to_string();
            validate_relative_path(&relative)?;
            let bytes = std::fs::read(&path)
                .map_err(|e| format!("reading staged file {}: {e}", path.display()))?;
            out.insert(relative, SecretBytes::from(bytes));
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
mod tests {
    use super::*;

    #[test]
    fn validate_relative_path_rejects_bad_inputs() {
        for path in ["", "/abs", "a//b", "a/./b", "a/../b", "a\0b"] {
            assert!(
                validate_relative_path(path).is_err(),
                "path {path:?} should fail"
            );
        }
        assert!(validate_relative_path(".config/gws").is_ok());
    }

    #[test]
    fn collect_staging_files_round_trips_files_section() {
        let tempdir = tempfile::TempDir::new().unwrap();
        let staging = tempdir.path().join("dir-0");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::create_dir_all(staging.join("sub")).unwrap();
        std::fs::write(staging.join("a.txt"), b"alpha").unwrap();
        std::fs::write(staging.join("sub").join("b.bin"), b"\x00\x01\x02").unwrap();

        let directory = MountedDirectory {
            resource_id: "test".to_string(),
            mounts: Vec::new(),
            staging_path: staging.clone(),
        };
        let files = collect_staging_files(&directory).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files.get("a.txt").unwrap().as_slice(), b"alpha");
        assert_eq!(files.get("sub/b.bin").unwrap().as_slice(), b"\x00\x01\x02");
    }
}
