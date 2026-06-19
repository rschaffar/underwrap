//! Per-(instance, wrapper) uid isolation (story 2600, re-keyed in 2720).
//!
//! Reads the root-owned authority table and resolves the dedicated sandbox
//! identity for the `(instance, wrapper)` pair `underwrap-ns` is about to run,
//! then performs (and verifies) the switch to it.
//!
//! The uid is taken **only** from the root-owned table — never from the
//! `ExecBundle`, the daemon response, or the target path. The lookup *key* is
//! the `(instance, wrapper)` selector underwrap-exec passes via env (the
//! kernel-enforced instance gate + the daemon-validated wrapper name); the
//! resolved uid/gid is never relayed. This removes the escalation /
//! confused-deputy vector structurally: the only uids reachable are the sandbox
//! uids trusted root config assigned, and the worst a compromised exec stage can
//! do is select a *different* legitimate sandbox uid (never root, never an
//! arbitrary number — see the floor / `gid != 0` checks below).

use std::ffi::CString;

/// Root-owned, non-secret authority table written by the NixOS module. One row
/// per `(instance, wrapper)` pair: `"<instance>\t<wrapper>\t<user>\t<group>"`.
/// Read directly here, keyed on the daemon-validated `(instance, wrapper)`
/// selector.
const TARGET_UID_TABLE: &str = "/etc/underwrap/target-uids";

/// Lowest uid we will ever switch a target to. `0` is root (escalation). The
/// dedicated sandbox users are auto-allocated system users, so excluding `0` is
/// the load-bearing check; the floor is defense-in-depth.
const MIN_TARGET_UID: libc::uid_t = 1;

/// The dedicated sandbox identity a target is isolated to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Isolation {
    pub uid: libc::uid_t,
    pub gid: libc::gid_t,
}

/// Resolve the dedicated sandbox identity for the `(instance, wrapper)` pair
/// from the root-owned table.
///
/// Under the mandatory-isolation model, both a missing table and a missing row
/// are hard errors: underwrap must never fall back to running the target as the
/// caller.
pub(crate) fn resolve_isolation(instance: &str, wrapper: &str) -> Result<Isolation, String> {
    resolve_isolation_from(TARGET_UID_TABLE, instance, wrapper)
}

fn resolve_isolation_from(
    table_path: &str,
    instance: &str,
    wrapper: &str,
) -> Result<Isolation, String> {
    let contents = match std::fs::read_to_string(table_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!(
                "mandatory target isolation table {table_path} is missing; refusing to run {instance}/{wrapper} as the caller"
            ));
        }
        Err(e) => return Err(format!("reading {table_path}: {e}")),
    };

    for (idx, line) in contents.lines().enumerate() {
        let lineno = idx + 1;
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let row_instance = parts.next().unwrap_or("");
        let row_wrapper = parts.next().unwrap_or("");
        let user = parts.next();
        let group = parts.next();
        if parts.next().is_some() {
            return Err(format!(
                "{table_path}:{lineno}: malformed row (more than 4 tab-separated fields)"
            ));
        }
        if row_instance != instance || row_wrapper != wrapper {
            continue;
        }
        let (user, group) = match (user, group) {
            (Some(u), Some(g)) if !u.is_empty() && !g.is_empty() => (u, g),
            _ => {
                return Err(format!(
                    "{table_path}:{lineno}: row for {instance}/{wrapper} is missing user/group"
                ));
            }
        };
        let uid = resolve_uid(user)?;
        let gid = resolve_gid(group)?;
        if uid < MIN_TARGET_UID {
            return Err(format!(
                "refusing to isolate {instance}/{wrapper} to uid {uid} (user {user:?}): below the \
                 minimum target uid {MIN_TARGET_UID} — never run a target as root or a \
                 privileged id"
            ));
        }
        if gid == 0 {
            return Err(format!(
                "refusing to isolate {instance}/{wrapper} to gid 0 (group {group:?})"
            ));
        }
        return Ok(Isolation { uid, gid });
    }
    Err(format!(
        "mandatory target isolation table {table_path} has no row for {instance}/{wrapper}; refusing to run as the caller"
    ))
}

/// Resolve a uid to its user name via `getpwuid`. Used to derive the executing
/// target's own runtime-dir path component — the NixOS module provisions that
/// dir keyed on the target user *name*, so ns resolves the same name from X.
pub(crate) fn user_name(uid: libc::uid_t) -> Result<String, String> {
    // SAFETY: getpwuid returns a pointer into static storage or NULL.
    let pw = unsafe { libc::getpwuid(uid) };
    if pw.is_null() {
        return Err(format!("getpwuid({uid}) returned null (no such uid)"));
    }
    // SAFETY: pw is non-null; pw_name is a valid NUL-terminated C string.
    let name = unsafe { std::ffi::CStr::from_ptr((*pw).pw_name) };
    name.to_str()
        .map(str::to_string)
        .map_err(|e| format!("user name for uid {uid} is not valid UTF-8: {e}"))
}

fn resolve_uid(name: &str) -> Result<libc::uid_t, String> {
    let c = CString::new(name).map_err(|_| format!("user name {name:?} has an interior NUL"))?;
    // SAFETY: `c` is a valid NUL-terminated string; getpwnam returns a pointer
    // into static storage or NULL.
    let pw = unsafe { libc::getpwnam(c.as_ptr()) };
    if pw.is_null() {
        return Err(format!("getpwnam({name:?}) returned null (no such user)"));
    }
    Ok(unsafe { (*pw).pw_uid })
}

fn resolve_gid(name: &str) -> Result<libc::gid_t, String> {
    let c = CString::new(name).map_err(|_| format!("group name {name:?} has an interior NUL"))?;
    // SAFETY: as above for getgrnam.
    let gr = unsafe { libc::getgrnam(c.as_ptr()) };
    if gr.is_null() {
        return Err(format!("getgrnam({name:?}) returned null (no such group)"));
    }
    Ok(unsafe { (*gr).gr_gid })
}

/// Switch the calling process to the dedicated sandbox identity `iso`.
///
/// gid before uid (so we still hold privilege to set the group ids), restricting
/// supplementary groups to exactly the target's primary gid. Uses
/// `setresgid`/`setresuid` (all three ids), checks every return, and bails on
/// any failure — NEVER exec a target that failed to drop to X.
///
/// Capabilities are intentionally RETAINED across this switch: with no uid `0`
/// on either side the kernel's setuid cap-fixup does not fire
/// (`security/commoncap.c`), so the child keeps `CAP_SYS_ADMIN` for any
/// remaining mount work. The caller MUST `drop_caps()` afterwards and
/// [`verify_switched`] before exec.
pub(crate) fn switch_to(iso: Isolation) -> Result<(), String> {
    let Isolation { uid, gid } = iso;
    let groups = [gid];
    if unsafe { libc::setgroups(1, groups.as_ptr()) } != 0 {
        return Err(format!(
            "setgroups([{gid}]) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if unsafe { libc::setresgid(gid, gid, gid) } != 0 {
        return Err(format!(
            "setresgid({gid},{gid},{gid}) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if unsafe { libc::setresuid(uid, uid, uid) } != 0 {
        return Err(format!(
            "setresuid({uid},{uid},{uid}) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Verify the identity switch + capability drop are real before exec (the
/// "Setuid Demystified" check). Asserts the **userns-invariant** properties so
/// it stays correct if a user namespace is added later (2650): all three uids
/// equal and non-zero, all three gids equal, and a regain attempt fails. The
/// literal "real uid == X, never 0/caller" check is the VM test's job (root
/// reads `/proc/<pid>/status` from outside).
pub(crate) fn verify_switched() -> Result<(), String> {
    let (mut r, mut e, mut s): (libc::uid_t, libc::uid_t, libc::uid_t) = (0, 0, 0);
    if unsafe { libc::getresuid(&mut r, &mut e, &mut s) } != 0 {
        return Err(format!(
            "getresuid failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if !(r == e && e == s) {
        return Err(format!(
            "uid not fully switched: real={r} eff={e} saved={s}"
        ));
    }
    if r == 0 {
        return Err("post-switch uid is 0 (root) — refusing to exec".to_string());
    }
    let (mut gr, mut ge, mut gs): (libc::gid_t, libc::gid_t, libc::gid_t) = (0, 0, 0);
    if unsafe { libc::getresgid(&mut gr, &mut ge, &mut gs) } != 0 {
        return Err(format!(
            "getresgid failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if !(gr == ge && ge == gs) {
        return Err(format!(
            "gid not fully switched: real={gr} eff={ge} saved={gs}"
        ));
    }
    // With caps cleared, a regain attempt MUST fail: setresuid(0,0,0) from a
    // non-root uid without CAP_SETUID returns EPERM.
    if unsafe { libc::setresuid(0, 0, 0) } == 0 {
        return Err(
            "SECURITY: setresuid(0,0,0) succeeded after the drop — capabilities were not \
             fully cleared; refusing to exec"
                .to_string(),
        );
    }
    Ok(())
}

/// Run `f` with the process's **filesystem** uid/gid temporarily set to the
/// isolated target's `iso`, then restore them — the single `setfsuid(X)` window
/// the supervisor uses to read back an isolated target's X-owned staging /
/// libsecret state at writeback (story 2600 / 2635).
///
/// `setfsuid`/`setfsgid` change only the credentials used for filesystem access
/// checks; they do not affect process ownership, signal permissions, or the
/// channel IPC (which operates on already-open fds). Entering the window needs
/// `CAP_SETUID`/`CAP_SETGID` (the target id is not the caller's own); the
/// supervisor retains those caps until after writeback for exactly this reason.
/// Restoring to the caller's own ids needs no capability.
///
/// The window doubles as the symlink-TOCTOU defense: while the supervisor walks
/// the target-controlled staging tree, its authority is bounded to uid X.
///
/// `f`'s value is returned on success. If restoring the ids fails the error is
/// surfaced (a leaked fsuid would be a security problem) and `f`'s value is
/// dropped.
pub(crate) fn with_target_fsuid<T>(iso: Isolation, f: impl FnOnce() -> T) -> Result<T, String> {
    let caller_uid = unsafe { libc::getuid() };
    let caller_gid = unsafe { libc::getgid() };
    enter_fsids(iso.uid, iso.gid)?;
    let result = f();
    restore_fsids(caller_uid, caller_gid)?;
    Ok(result)
}

/// Set fsgid then fsuid to the target ids, verifying each took effect.
/// `setfsuid`/`setfsgid` report no error directly (they return the *previous*
/// id), so a follow-up query call (`-1`) confirms the change landed.
fn enter_fsids(uid: libc::uid_t, gid: libc::gid_t) -> Result<(), String> {
    // gid first, mirroring the gid-before-uid discipline of the real switch.
    unsafe { libc::setfsgid(gid) };
    if unsafe { libc::setfsgid(libc::gid_t::MAX) } as libc::gid_t != gid {
        return Err(format!(
            "setfsgid({gid}) did not take effect (missing CAP_SETGID?)"
        ));
    }
    unsafe { libc::setfsuid(uid) };
    if unsafe { libc::setfsuid(libc::uid_t::MAX) } as libc::uid_t != uid {
        // Roll the gid back before bailing so we never leave a half-applied window.
        let cur_gid = unsafe { libc::getgid() };
        unsafe { libc::setfsgid(cur_gid) };
        return Err(format!(
            "setfsuid({uid}) did not take effect (missing CAP_SETUID?)"
        ));
    }
    Ok(())
}

/// Restore the filesystem ids to the caller's own uid/gid (no capability
/// needed) and verify. A failure here is fatal to the run.
fn restore_fsids(uid: libc::uid_t, gid: libc::gid_t) -> Result<(), String> {
    unsafe { libc::setfsuid(uid) };
    if unsafe { libc::setfsuid(libc::uid_t::MAX) } as libc::uid_t != uid {
        return Err(format!("failed to restore fsuid to {uid}"));
    }
    unsafe { libc::setfsgid(gid) };
    if unsafe { libc::setfsgid(libc::gid_t::MAX) } as libc::gid_t != gid {
        return Err(format!("failed to restore fsgid to {gid}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
