//! Idmapped bind mounts (story 2600 / 2640).
//!
//! Give an isolated (dedicated-uid X) target access to a configured set of the
//! caller's directories by over-mounting each with an idmapped clone whose
//! user namespace maps EXACTLY caller<->X (one uid, one gid). Inside such a
//! mount, files owned by the caller present as owned by X — the target reads and
//! writes them as the caller, and its writes land back on disk owned by the
//! caller — while every other owner is unmapped (overflow) and left untouched.
//!
//! Runs in the parent, BEFORE the fork, while fsuid is still the caller (so
//! `open_tree` can traverse the caller's 0700 home as its owner) and
//! CAP_SYS_ADMIN / CAP_SETUID / CAP_SETGID are still held (for
//! `mount_setattr`/`move_mount` and the single-entry uid_map write). The mounts
//! land in the already-unshared private mount namespace, so they are invisible
//! to the host and to concurrent invocations; the forked child (which becomes X)
//! inherits them.

use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Component, Path, PathBuf};

use crate::target_uid::Isolation;

/// `open_tree(2)` flag: set close-on-exec on the returned fd.
const OPEN_TREE_CLOEXEC: libc::c_uint = libc::O_CLOEXEC as libc::c_uint;

/// Resolve one configured idmap spec to an absolute path.
///
/// `~` or `~/<rel>` resolves against `caller_home`; anything else must already
/// be absolute. Rejects empty specs, `~user` forms, and any `..` traversal.
pub(crate) fn resolve_spec(spec: &str, caller_home: &Path) -> Result<PathBuf, String> {
    if spec.is_empty() {
        return Err("empty idmap path".to_string());
    }
    let path = if spec == "~" {
        caller_home.to_path_buf()
    } else if let Some(rel) = spec.strip_prefix("~/") {
        caller_home.join(rel)
    } else if spec.starts_with('~') {
        return Err(format!(
            "idmap path {spec:?}: the ~user form is not supported"
        ));
    } else if spec.starts_with('/') {
        PathBuf::from(spec)
    } else {
        return Err(format!(
            "idmap path {spec:?} must be absolute or start with '~/'"
        ));
    };
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("idmap path {spec:?} must not contain '..'"));
    }
    Ok(path)
}

/// Mount idmapped clones of every configured directory, using a userns fd from
/// [`create_idmap_userns`]. Call this AFTER the private mount namespace exists
/// (so the mounts are namespace-local and invisible to the host) but while fsuid
/// is still the caller (so `open_tree` traverses the caller's home as owner).
pub(crate) fn mount_idmapped_dirs(
    specs: &[String],
    caller_home: &Path,
    userns: &OwnedFd,
) -> Result<(), String> {
    for spec in specs {
        let dir = resolve_spec(spec, caller_home)?;
        // Create the directory (as the caller, owned by the caller) if missing,
        // so a not-yet-created config dir still gets idmapped. Existing dirs are
        // a no-op; failures surface from open_tree below.
        let _ = std::fs::create_dir_all(&dir);
        idmap_dir(&dir, userns.as_raw_fd())
            .map_err(|e| format!("idmapping {}: {e}", dir.display()))?;
    }
    Ok(())
}

/// Build a user-namespace fd whose only mapping is caller<->X (uid and gid),
/// for use as a mount idmapping.
///
/// Forks a short-lived child that unshares a user namespace and blocks; the
/// PARENT writes the child's single-entry maps. The child cannot write its own
/// maps (inside its fresh, still-unmapped userns it is the overflow id), but its
/// `/proc/<pid>/*` map files are owned by the parent's uid, so the parent can.
/// The map needs `CAP_SETUID`/`CAP_SETGID` (the presented id is not the writer's
/// own id), which the parent holds at this point. The parent then opens the
/// child's `/proc/<pid>/ns/user` for the mount idmapping; the userns stays alive
/// via that fd after the child is reaped.
///
/// The map line `"<caller> <X> 1"` (filesystem id, presented id) means: a file
/// owned by `caller` on the underlying filesystem presents as owned by `X`
/// through the idmapped mount, and a file `X` creates lands on disk owned by
/// `caller` (`make_vfsuid` = `map_id_down`: first->lower_first).
///
/// **Call this BEFORE unsharing the PID namespace.** The helper fork would
/// otherwise become PID 1 of the new PID namespace and, once reaped, leave that
/// namespace unable to host the real target fork.
pub(crate) fn create_idmap_userns(
    caller_uid: libc::uid_t,
    caller_gid: libc::gid_t,
    iso: Isolation,
) -> Result<OwnedFd, String> {
    // The map lines, in the kernel's idmapped-mount sense: each line is
    // "<filesystem id> <presented id> <count>". `make_vfsuid` does
    // `map_id_down`, which looks the on-disk id up in the FIRST column and
    // returns the LOWER_first column — so the on-disk caller id must be FIRST and
    // the presented target id LOWER. A file owned by `caller` on disk then
    // presents as owned by the target uid through the mount, and the target's
    // writes (target id, mapped back via `map_id_up`) land on disk owned by the
    // caller. This is NOT the writer's own id in the LOWER column, so it needs
    // CAP_SETUID/CAP_SETGID — which the parent holds — and the PARENT writes the
    // child's map files (the child, unmapped inside its fresh userns, cannot
    // write its own; the files are owned by the parent's uid).
    let uid_map = format!("{} {} 1", caller_uid, iso.uid);
    let gid_map = format!("{} {} 1", caller_gid, iso.gid);

    // `ready`: child -> parent ("I have unshared"); `release`: parent -> child.
    let mut ready = [0i32; 2];
    let mut release = [0i32; 2];
    if unsafe { libc::pipe(ready.as_mut_ptr()) } != 0 {
        return Err(format!("pipe failed: {}", std::io::Error::last_os_error()));
    }
    if unsafe { libc::pipe(release.as_mut_ptr()) } != 0 {
        return Err(format!("pipe failed: {}", std::io::Error::last_os_error()));
    }

    // SAFETY: underwrap-ns is single-threaded here, so a fork with only
    // async-signal-safe libc calls in the child is sound.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(format!("fork failed: {}", std::io::Error::last_os_error()));
    }
    if pid == 0 {
        // SAFETY: async-signal-safe path only — unshare + write + read.
        unsafe {
            libc::close(ready[0]);
            libc::close(release[1]);
            let ok = libc::unshare(libc::CLONE_NEWUSER) == 0;
            let msg = [u8::from(ok)];
            libc::write(ready[1], msg.as_ptr() as *const libc::c_void, 1);
            // Block until the parent has written the maps + opened the ns fd.
            let mut buf = [0u8; 1];
            while libc::read(release[0], buf.as_mut_ptr() as *mut libc::c_void, 1) > 0 {}
            libc::_exit(if ok { 0 } else { 1 });
        }
    }

    unsafe {
        libc::close(ready[1]);
        libc::close(release[0]);
    }
    // Release the child + reap it, then return the result, regardless of outcome.
    let finish = |result: Result<OwnedFd, String>| -> Result<OwnedFd, String> {
        unsafe {
            libc::close(release[1]);
            let mut st = 0;
            libc::waitpid(pid, &mut st, 0);
            libc::close(ready[0]);
        }
        result
    };

    let mut buf = [0u8; 1];
    let n = unsafe { libc::read(ready[0], buf.as_mut_ptr() as *mut libc::c_void, 1) };
    if n != 1 || buf[0] != 1 {
        return finish(Err(
            "idmap helper failed to unshare a user namespace".to_string()
        ));
    }
    // setgroups must be denied before gid_map (no CAP_SETGID in the parent ns).
    if let Err(e) = write_proc(pid, "setgroups", "deny") {
        return finish(Err(e));
    }
    if let Err(e) = write_proc(pid, "uid_map", &uid_map) {
        return finish(Err(e));
    }
    if let Err(e) = write_proc(pid, "gid_map", &gid_map) {
        return finish(Err(e));
    }
    let path = format!("/proc/{pid}/ns/user");
    match std::fs::File::open(&path) {
        Ok(f) => finish(Ok(OwnedFd::from(f))),
        Err(e) => finish(Err(format!("opening {path}: {e}"))),
    }
}

/// Write `contents` to `/proc/<pid>/<name>` (the parent writing a child's
/// userns map files).
fn write_proc(pid: libc::pid_t, name: &str, contents: &str) -> Result<(), String> {
    use std::io::Write;
    let path = format!("/proc/{pid}/{name}");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .map_err(|e| format!("opening {path}: {e}"))?;
    f.write_all(contents.as_bytes())
        .map_err(|e| format!("writing {path}: {e}"))
}

/// `open_tree` (recursive clone) + best-effort per-mount
/// `mount_setattr(IDMAP|NOSUID|NODEV)` + `move_mount` for one dir.
///
/// The clone is RECURSIVE so submounts under `dir` (e.g. nested ZFS datasets or
/// bind mounts inside the caller's `~`) come along and can be idmapped too. The
/// idmap itself is applied **per mount, not recursively**: a recursive
/// `mount_setattr(AT_RECURSIVE)` is all-or-nothing and returns `EINVAL` for the
/// whole tree the moment it meets a submount whose filesystem lacks
/// `FS_ALLOW_IDMAP` (notably fuse — xdg-document-portal, gvfsd-fuse, sshfs,
/// AppImage). Applying it per mount lets us idmap every filesystem that supports
/// it (ext4/xfs/btrfs/zfs ≥ OpenZFS 2.2/…) and **skip** the ones that don't,
/// with a warning, instead of failing the whole access path.
///
/// The named directory's own filesystem MUST support idmapping — if it does not
/// the access path is pointless, so that is a hard error. Only submounts are
/// best-effort: a skipped submount stays in the tree un-idmapped, so its files
/// present as their on-disk (caller) owner and are simply not caller-mapped for
/// the target — never a failure, never an exposure.
fn idmap_dir(dir: &Path, userns_fd: libc::c_int) -> Result<(), String> {
    let c_dir = CString::new(dir.as_os_str().as_encoded_bytes())
        .map_err(|_| "directory path has an interior NUL".to_string())?;
    let empty = CString::new("").unwrap();

    let flags = libc::OPEN_TREE_CLONE | OPEN_TREE_CLOEXEC | (libc::AT_RECURSIVE as libc::c_uint);
    // SAFETY: open_tree(AT_FDCWD, path, flags) returns a detached-mount fd or -1.
    let tree_fd = unsafe {
        libc::syscall(
            libc::SYS_open_tree,
            libc::AT_FDCWD as libc::c_long,
            c_dir.as_ptr(),
            flags as libc::c_long,
        )
    };
    if tree_fd < 0 {
        return Err(format!("open_tree: {}", std::io::Error::last_os_error()));
    }
    // SAFETY: tree_fd is a fresh, owned fd from open_tree.
    let tree = unsafe { OwnedFd::from_raw_fd(tree_fd as libc::c_int) };

    let attr = libc::mount_attr {
        attr_set: libc::MOUNT_ATTR_IDMAP | libc::MOUNT_ATTR_NOSUID | libc::MOUNT_ATTR_NODEV,
        attr_clr: 0,
        propagation: 0,
        userns_fd: userns_fd as u64,
    };

    // Idmap the named directory's own mount (the root of the detached tree).
    // AT_EMPTY_PATH targets the tree fd itself; no AT_RECURSIVE, so only this
    // one mount. A non-idmappable root is fatal — the access path cannot work.
    match set_mount_idmap(tree.as_raw_fd(), empty.as_ptr(), libc::AT_EMPTY_PATH, &attr) {
        Ok(()) => {}
        Err(errno) if errno == libc::EINVAL => {
            return Err(format!(
                "{}: filesystem does not support idmapped mounts (FS_ALLOW_IDMAP); \
                 cannot grant the target caller-owned access to it",
                dir.display()
            ));
        }
        Err(errno) => {
            return Err(format!(
                "mount_setattr(IDMAP) on {}: {}",
                dir.display(),
                std::io::Error::from_raw_os_error(errno)
            ));
        }
    }

    // Best-effort idmap each submount carried in by the recursive clone. We
    // address each by its path relative to `dir` (from /proc/self/mountinfo, read
    // in this already-private mount namespace). ANY failure to idmap a submount is
    // skipped with a warning, never fatal: a submount can be un-idmappable for
    // several reasons — its filesystem lacks FS_ALLOW_IDMAP (fuse → EINVAL), or
    // its superblock is owned by a foreign user namespace we are not privileged
    // over (e.g. a Steam pressure-vessel / sandbox mount → EACCES/EPERM). In every
    // case skipping only leaves that subtree un-idmapped (its files present as
    // their on-disk owner, simply not caller-mapped for the target) — never a
    // failure of the whole access path, and never an exposure to the caller.
    for rel in submount_relpaths(dir) {
        let c_rel = match CString::new(rel.as_os_str().as_encoded_bytes()) {
            Ok(c) => c,
            Err(_) => continue, // interior NUL: cannot address it; leave un-idmapped.
        };
        if let Err(errno) = set_mount_idmap(tree.as_raw_fd(), c_rel.as_ptr(), 0, &attr) {
            eprintln!(
                "underwrap-ns: skipping submount {} under {} — could not idmap it \
                 ({}); the target will not see it as the caller",
                rel.display(),
                dir.display(),
                std::io::Error::from_raw_os_error(errno)
            );
        }
    }

    // SAFETY: move_mount attaches the detached tree (with its idmapped submounts)
    // over `dir`.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_move_mount,
            tree.as_raw_fd() as libc::c_long,
            empty.as_ptr(),
            libc::AT_FDCWD as libc::c_long,
            c_dir.as_ptr(),
            libc::MOVE_MOUNT_F_EMPTY_PATH as libc::c_long,
        )
    };
    if ret != 0 {
        return Err(format!("move_mount: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

/// `mount_setattr` on one mount within the detached tree. `path`/`at_flags`
/// select the mount: `("", AT_EMPTY_PATH)` for the tree root, or a relative path
/// with `0` for a submount. Never recursive. Returns the raw errno on failure so
/// the caller can distinguish `EINVAL` (filesystem lacks `FS_ALLOW_IDMAP`).
fn set_mount_idmap(
    tree_fd: libc::c_int,
    path: *const libc::c_char,
    at_flags: libc::c_int,
    attr: &libc::mount_attr,
) -> Result<(), i32> {
    // SAFETY: mount_setattr on the detached tree fd with a valid mount_attr.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_mount_setattr,
            tree_fd as libc::c_long,
            path,
            at_flags as libc::c_long,
            attr as *const libc::mount_attr,
            std::mem::size_of::<libc::mount_attr>() as libc::c_long,
        )
    };
    if ret != 0 {
        return Err(std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO));
    }
    Ok(())
}

/// Mountpoints strictly under `dir` in the current (private) mount namespace,
/// as paths relative to `dir`. Reads `/proc/self/mountinfo`; returns an empty
/// list if it cannot be read (best-effort — the root mount is idmapped
/// regardless).
fn submount_relpaths(dir: &Path) -> Vec<PathBuf> {
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    // Canonicalize so the prefix match lines up with the kernel's canonical
    // mountpoint paths. Fall back to the given path if canonicalization fails.
    let base = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    submounts_under(&mountinfo, &base)
}

/// Pure parser: relative mountpoints strictly under `base`, parsed from
/// `mountinfo` contents. Field 5 (1-indexed) of each line is the mount point,
/// with spaces/tabs/newlines/backslashes octal-escaped (`\040` etc.).
fn submounts_under(mountinfo: &str, base: &Path) -> Vec<PathBuf> {
    let mut rels = Vec::new();
    for line in mountinfo.lines() {
        let Some(field) = line.split(' ').nth(4) else {
            continue;
        };
        let mountpoint = unescape_mountinfo(field);
        let mp = Path::new(&mountpoint);
        if let Ok(rel) = mp.strip_prefix(base)
            && !rel.as_os_str().is_empty()
        {
            rels.push(rel.to_path_buf());
        }
    }
    // Shallowest first (parents before children); not required for correctness
    // since each idmap is independent, but tidy and deterministic.
    rels.sort();
    rels
}

/// Decode mountinfo octal escapes (`\040` space, `\011` tab, `\012` newline,
/// `\134` backslash). Anything that is not a well-formed `\ooo` escape is copied
/// through verbatim.
fn unescape_mountinfo(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 4], 8)
        {
            out.push(v);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests;
