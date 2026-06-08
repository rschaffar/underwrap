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
use std::ffi::{CStr, CString};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource_blob::ResourceBlob;

/// The pure layout for one files-slot mount: where its staging tmpfs
/// subdirectory lives and where it binds into the target's home. Computed by
/// the parent with **no filesystem side effects** (see
/// [`plan_directory_mounts`]) so the same layout drives both the realize step
/// — which runs in the parent for an unisolated target but in the child (at
/// fsuid X) for an isolated one — and the end-of-run writeback walk the parent
/// performs (under a `setfsuid(X)` window when isolated).
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

/// One realized bind-mount, carrying only what cleanup needs: the mountpoint to
/// unmount and the directories created on the way to it. Only the **unisolated**
/// path produces these in the parent and cleans them up explicitly; an isolated
/// target's mounts are realized in the child and torn down with the private
/// mount namespace at parent exit.
pub struct DirectoryMount {
    /// Absolute mountpoint inside the namespace, `<target_home>/<mount_path>`.
    pub mountpoint: PathBuf,
    /// Directories created on the way to `mountpoint` (zero to N
    /// entries, including `mountpoint` itself iff it had to be
    /// created). Cleanup `rmdir`s these in reverse order — entries
    /// that already existed are not in this list and stay put.
    pub created_dirs: Vec<PathBuf>,
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
/// already be mounted (by the parent for an unisolated target, or by the child
/// after switching to uid X for an isolated one — see
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
) -> Result<Vec<DirectoryMount>, String> {
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

    // Phase 3: bind-mount each staging dir at its target path and
    // emit the final `DirectoryMount` records.
    let mut mounts: Vec<DirectoryMount> = Vec::with_capacity(plans.len());
    for plan in plans {
        let (mountpoint, created_dirs) = ensure_mount_target(target_home, &plan.mount_path)?;
        bind_mount_dir(&plan.staging_path, &mountpoint)?;
        mounts.push(DirectoryMount {
            mountpoint,
            created_dirs,
        });
    }

    Ok(mounts)
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
/// `hardened` selects the symlink-TOCTOU-resistant walk used for an isolated
/// target (the staging is target-controlled and the supervisor reads it under a
/// `setfsuid(X)` window): every path is resolved with
/// `openat2(RESOLVE_NO_SYMLINKS | RESOLVE_BENEATH)` and each entry is opened
/// `O_NOFOLLOW | O_NONBLOCK` so a planted symlink or FIFO cannot redirect or
/// stall the read. The unisolated walk reads the caller's own files, where a
/// path-based walk that rejects symlinks is sufficient (no privilege to abuse).
pub fn collect_staging_files(
    plans: &[DirectoryMountPlan],
    hardened: bool,
) -> Result<BTreeMap<String, SecretBytes>, String> {
    let mut out = BTreeMap::new();
    for plan in plans {
        if hardened {
            collect_files_hardened(&plan.staging_path, &plan.mount_path, &mut out)?;
        } else {
            collect_files_with_prefix(
                &plan.staging_path,
                &plan.staging_path,
                &plan.mount_path,
                &mut out,
            )?;
        }
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

pub fn cleanup_directory_mounts(mounts: &[DirectoryMount]) -> Result<(), String> {
    let mut first_error = None;
    for mount in mounts.iter().rev() {
        if let Err(e) = unmount_path(&mount.mountpoint) {
            first_error.get_or_insert(e);
            continue;
        }
        for dir in mount.created_dirs.iter().rev() {
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

// --- Hardened (openat2-rooted) staging walk for isolated targets ---
//
// The supervisor walks a *target-controlled* staging tree under a
// `setfsuid(X)` window (story 2600 / 2635). The window already bounds the
// supervisor's authority to uid X, but a lingering uid-X descendant could race
// the walk; openat2 with `RESOLVE_NO_SYMLINKS | RESOLVE_BENEATH` closes the
// symlink-TOCTOU and subtree-escape vectors as a second layer.

/// `openat2(2)` resolve flags (uapi/linux/openat2.h).
const RESOLVE_NO_SYMLINKS: u64 = 0x04;
const RESOLVE_BENEATH: u64 = 0x08;

#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

/// `openat2(dirfd, path, { flags, resolve })`, returning an owned fd.
fn openat2(
    dirfd: libc::c_int,
    path: &CStr,
    flags: libc::c_int,
    resolve: u64,
) -> std::io::Result<OwnedFd> {
    let how = OpenHow {
        flags: flags as u64,
        mode: 0,
        resolve,
    };
    // SAFETY: openat2 reads `how` for the given length and returns a new fd or
    // -1; `path` is a valid NUL-terminated string.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            dirfd as libc::c_long,
            path.as_ptr(),
            &how as *const OpenHow,
            std::mem::size_of::<OpenHow>() as libc::c_long,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `fd` is a fresh, owned descriptor returned by openat2.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) })
}

/// Hardened counterpart to [`collect_files_with_prefix`]: open the staging root
/// and recurse using fd-relative `openat2`, refusing symlinks and any escape
/// from the staging subtree.
fn collect_files_hardened(
    staging_root: &Path,
    mount_prefix: &str,
    out: &mut BTreeMap<String, SecretBytes>,
) -> Result<(), String> {
    let root_c = path_to_cstring(staging_root)?;
    let root_fd = openat2(
        libc::AT_FDCWD,
        &root_c,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        RESOLVE_NO_SYMLINKS,
    )
    .map_err(|e| format!("opening staging dir {}: {e}", staging_root.display()))?;
    walk_dir_fd(&root_fd, staging_root, mount_prefix, "", out)
}

/// Enumerate `dir_fd`'s entries and recurse. `relative` is the path of `dir_fd`
/// relative to the staging root (empty at the root); emitted keys are
/// `"{mount_prefix}/{entry-relative-to-root}"`.
fn walk_dir_fd(
    dir_fd: &OwnedFd,
    dir_display: &Path,
    mount_prefix: &str,
    relative: &str,
    out: &mut BTreeMap<String, SecretBytes>,
) -> Result<(), String> {
    // `fdopendir` takes ownership of the fd it is handed and `closedir` closes
    // it, so give it a dup and keep `dir_fd` itself for the per-entry openat2
    // anchor calls (openat2 ignores the directory stream's file position).
    let dup_fd = unsafe { libc::dup(dir_fd.as_raw_fd()) };
    if dup_fd < 0 {
        return Err(format!(
            "dup staging dir fd for {}: {}",
            dir_display.display(),
            std::io::Error::last_os_error()
        ));
    }
    let dirp = unsafe { libc::fdopendir(dup_fd) };
    if dirp.is_null() {
        let err = std::io::Error::last_os_error();
        unsafe { libc::close(dup_fd) };
        return Err(format!("fdopendir {}: {err}", dir_display.display()));
    }
    let result = walk_dirp(dir_fd, dirp, dir_display, mount_prefix, relative, out);
    // closedir closes the underlying dup_fd.
    unsafe { libc::closedir(dirp) };
    result
}

fn walk_dirp(
    dir_fd: &OwnedFd,
    dirp: *mut libc::DIR,
    dir_display: &Path,
    mount_prefix: &str,
    relative: &str,
    out: &mut BTreeMap<String, SecretBytes>,
) -> Result<(), String> {
    loop {
        // Reset errno so a NULL return can be distinguished: end-of-stream
        // leaves errno unchanged, a read error sets it.
        unsafe { *libc::__errno_location() = 0 };
        let ent = unsafe { libc::readdir(dirp) };
        if ent.is_null() {
            let errno = unsafe { *libc::__errno_location() };
            if errno != 0 {
                return Err(format!(
                    "readdir {}: {}",
                    dir_display.display(),
                    std::io::Error::from_raw_os_error(errno)
                ));
            }
            return Ok(());
        }
        // SAFETY: `ent` is a valid dirent for the lifetime until the next
        // readdir; `d_name` is NUL-terminated.
        let name_c = unsafe { CStr::from_ptr((*ent).d_name.as_ptr()) };
        let name_bytes = name_c.to_bytes();
        if name_bytes == b"." || name_bytes == b".." {
            continue;
        }
        let name = name_c
            .to_str()
            .map_err(|_| format!("non-UTF-8 entry name in {}", dir_display.display()))?;
        let entry_relative = if relative.is_empty() {
            name.to_string()
        } else {
            format!("{relative}/{name}")
        };
        let name_owned = CString::new(name_bytes).map_err(|_| {
            format!(
                "entry name in {} has an interior NUL",
                dir_display.display()
            )
        })?;

        // Open the single component relative to the directory fd, refusing
        // symlinks (RESOLVE_NO_SYMLINKS + O_NOFOLLOW) and subtree escapes
        // (RESOLVE_BENEATH); O_NONBLOCK so a planted FIFO cannot stall the open.
        let child = openat2(
            dir_fd.as_raw_fd(),
            &name_owned,
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            RESOLVE_NO_SYMLINKS | RESOLVE_BENEATH,
        )
        .map_err(|e| {
            format!(
                "opening staged entry {}/{name}: {e} (symlinks and subtree escapes are refused)",
                dir_display.display()
            )
        })?;

        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(child.as_raw_fd(), &mut st) } != 0 {
            return Err(format!(
                "fstat staged entry {}/{name}: {}",
                dir_display.display(),
                std::io::Error::last_os_error()
            ));
        }
        match st.st_mode & libc::S_IFMT {
            libc::S_IFDIR => {
                walk_dir_fd(
                    &child,
                    &dir_display.join(name),
                    mount_prefix,
                    &entry_relative,
                    out,
                )?;
            }
            libc::S_IFREG => {
                validate_relative_path(&entry_relative)?;
                let bytes = read_fd_to_end(child).map_err(|e| {
                    format!("reading staged file {}/{name}: {e}", dir_display.display())
                })?;
                out.insert(
                    format!("{mount_prefix}/{entry_relative}"),
                    SecretBytes::from(bytes),
                );
            }
            _ => {
                return Err(format!(
                    "staged entry {}/{name} is not a regular file or directory",
                    dir_display.display()
                ));
            }
        }
    }
}

fn read_fd_to_end(fd: OwnedFd) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut file = std::fs::File::from(fd);
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
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
