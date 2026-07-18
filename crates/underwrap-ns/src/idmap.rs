//! Idmapped bind mounts for explicit caller-owned path access.
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

/// The idmapping `mount_attr` for `userns_fd` (also NOSUID/NODEV-hardened).
fn idmap_attr(userns_fd: libc::c_int) -> libc::mount_attr {
    libc::mount_attr {
        attr_set: libc::MOUNT_ATTR_IDMAP | libc::MOUNT_ATTR_NOSUID | libc::MOUNT_ATTR_NODEV,
        attr_clr: 0,
        propagation: 0,
        userns_fd: userns_fd as u64,
    }
}

/// Idmap one directory and as many of its submounts as the kernel allows.
///
/// The kernel will only set an idmap on the ROOT of an `open_tree` clone (via
/// `AT_EMPTY_PATH`), and a recursive `mount_setattr(AT_RECURSIVE)` is
/// all-or-nothing — it fails for the whole tree the moment it meets a submount
/// whose filesystem lacks `FS_ALLOW_IDMAP` (fuse — portals, sshfs, AppImage) or
/// whose superblock is owned by a foreign user namespace (a sandbox /
/// pressure-vessel mount). So we cannot idmap submounts one-by-one (the kernel
/// rejects idmapping a non-root mount of a detached tree), and we cannot blindly
/// recurse over a tree that contains a non-idmappable mount.
///
/// Instead: first **detach the non-idmappable submounts from our private mount
/// namespace** (it is a throwaway clone, invisible to the host and the caller's
/// real session, so this is free and safe), then RECURSIVELY clone + idmap what
/// remains in one shot. That maps `dir` and every idmappable submount under it
/// (nested ZFS datasets ≥ OpenZFS 2.2, ext4/xfs/btrfs binds, …) while the
/// genuinely-non-idmappable mounts simply aren't present for the target — never a
/// failure of the access path, and never an exposure to the caller.
///
/// We classify each submount with a cheap probe (clone it standalone and try to
/// idmap that clone — the same operation that idmaps the root, which the kernel
/// *does* permit). If a non-idmappable submount cannot be detached, we fall back
/// to idmapping `dir`'s own mount only (no submounts) rather than fail.
///
/// The named directory's own filesystem MUST support idmapping — if it does not,
/// the access path is pointless, so that is a hard error.
fn idmap_dir(dir: &Path, userns_fd: libc::c_int) -> Result<(), String> {
    let c_dir = CString::new(dir.as_os_str().as_encoded_bytes())
        .map_err(|_| "directory path has an interior NUL".to_string())?;
    let empty = CString::new("").unwrap();
    let attr = idmap_attr(userns_fd);

    // Detach every non-idmappable submount from our private namespace so the
    // recursive idmap below can succeed. Deepest-first (reverse of the ascending
    // sort) so a child is detached before its parent. If any cannot be detached,
    // we cannot safely recurse — fall back to the root mount only.
    let submounts = submount_relpaths(dir);
    let mut recursive = true;
    for rel in submounts.iter().rev() {
        let abs = dir.join(rel);
        if is_idmappable(&abs, userns_fd) {
            continue;
        }
        if detach_submount(&abs) {
            eprintln!(
                "underwrap-ns: detached non-idmappable submount {} under {} \
                 (its filesystem does not support idmapped mounts; the target \
                 will not see it)",
                rel.display(),
                dir.display()
            );
        } else {
            recursive = false;
            eprintln!(
                "underwrap-ns: could not detach non-idmappable submount {} under \
                 {}; idmapping only {} itself (its submounts will not be visible \
                 to the target)",
                rel.display(),
                dir.display(),
                dir.display()
            );
        }
    }

    // Clone the subtree (recursively unless a blocker survived) and idmap it.
    // With AT_RECURSIVE the single mount_setattr idmaps `dir` AND every remaining
    // submount; without it, only `dir`'s own mount.
    let recursive_flag = if recursive {
        libc::AT_RECURSIVE as libc::c_uint
    } else {
        0
    };
    let flags = libc::OPEN_TREE_CLONE | OPEN_TREE_CLOEXEC | recursive_flag;
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

    let setattr_flags = libc::AT_EMPTY_PATH | if recursive { libc::AT_RECURSIVE } else { 0 };
    match set_mount_idmap(tree.as_raw_fd(), empty.as_ptr(), setattr_flags, &attr) {
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

/// Probe whether the mount at `abs_path` can be idmapped: clone it as a
/// standalone detached mount (non-recursive) and try to set the idmap on that
/// clone — the same `AT_EMPTY_PATH` operation the kernel permits on a tree root.
/// Returns true only if both the clone and the idmap succeed; the probe clone is
/// discarded (its fd is dropped) and the live mount is untouched. Runs at the
/// caller's fsuid while nothing is idmapped yet, so the path is traversable.
fn is_idmappable(abs_path: &Path, userns_fd: libc::c_int) -> bool {
    let Ok(c_path) = CString::new(abs_path.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    let empty = CString::new("").unwrap();
    let flags = libc::OPEN_TREE_CLONE | OPEN_TREE_CLOEXEC;
    // SAFETY: open_tree of an existing path; returns a detached-mount fd or -1.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_open_tree,
            libc::AT_FDCWD as libc::c_long,
            c_path.as_ptr(),
            flags as libc::c_long,
        )
    };
    if fd < 0 {
        return false;
    }
    // SAFETY: fd is a fresh, owned fd from open_tree; dropped at end of scope.
    let tree = unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) };
    let attr = idmap_attr(userns_fd);
    set_mount_idmap(tree.as_raw_fd(), empty.as_ptr(), libc::AT_EMPTY_PATH, &attr).is_ok()
}

/// Lazily detach the mount at `abs_path` from the current (private) namespace.
/// `MNT_DETACH` so a mount with children still comes off. Returns true on
/// success. Host mounts are unaffected — we are in an unshared, MS_PRIVATE ns.
fn detach_submount(abs_path: &Path) -> bool {
    let Ok(c_path) = CString::new(abs_path.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    // SAFETY: umount2 of an existing path with a valid flag.
    unsafe { libc::umount2(c_path.as_ptr(), libc::MNT_DETACH) == 0 }
}

/// `mount_setattr` on a mount selected by `path`/`at_flags`: `("", AT_EMPTY_PATH)`
/// for a detached tree's root (optionally `| AT_RECURSIVE` to cover its
/// submounts). Returns the raw errno on failure so callers can distinguish
/// `EINVAL` (filesystem lacks `FS_ALLOW_IDMAP`).
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
