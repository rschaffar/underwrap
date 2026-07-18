//! Target-to-caller boundary hardening.
//!
//! The target runs under a distinct UID while retaining caller-provided stdio.
//! These controls sever ambient session capabilities that should not cross that
//! boundary. They run in the child immediately before target exec.

/// `CLOSE_RANGE_CLOEXEC` (linux/close_range.h) — mark the range close-on-exec
/// rather than closing it immediately, so std's exec machinery is undisturbed.
const CLOSE_RANGE_CLOEXEC: libc::c_uint = 1 << 2;

/// `keyctl(2)` command: create and join a fresh anonymous session keyring.
const KEYCTL_JOIN_SESSION_KEYRING: libc::c_long = 1;

/// Sever the controlling terminal so a uid-X target cannot inject input into the
/// caller's shell via `TIOCSTI`/`TIOCLINUX` on a shared controlling tty. The
/// target keeps its inherited stdio fds; it only loses the *controlling*-tty
/// relationship. Fail closed — never exec across the boundary with the caller's
/// controlling tty still attached.
pub(crate) fn sever_controlling_tty() -> Result<(), String> {
    // SAFETY: setsid takes no arguments. The forked child is not a process-group
    // leader, so this succeeds and starts a new session with no controlling tty.
    if unsafe { libc::setsid() } == -1 {
        return Err(format!(
            "setsid failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Join a fresh anonymous session keyring so the uid-X target cannot read the
/// caller's session keyring (cached credentials). Best-effort: a kernel built
/// without keyrings (`ENOSYS`) is not a failure.
pub(crate) fn join_session_keyring() -> Result<(), String> {
    // SAFETY: keyctl(KEYCTL_JOIN_SESSION_KEYRING, NULL) creates and joins an
    // anonymous session keyring for the calling process.
    let ret = unsafe { libc::syscall(libc::SYS_keyctl, KEYCTL_JOIN_SESSION_KEYRING, 0_i64) };
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ENOSYS) {
            return Ok(());
        }
        return Err(format!("keyctl(KEYCTL_JOIN_SESSION_KEYRING) failed: {err}"));
    }
    Ok(())
}

/// Mark every inherited fd >= 3 close-on-exec, so no privileged fd (the resource
/// channel, helper pipes) leaks across the exec into the uid-X target (the Leaky
/// Vessels / CVE-2024-21626 class). stdio (0, 1, 2) is preserved.
pub(crate) fn close_inherited_fds_for_exec() -> Result<(), String> {
    // SAFETY: close_range(3, ~0, CLOSE_RANGE_CLOEXEC) only flips the CLOEXEC flag
    // on the given fd range; it creates/closes nothing.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_close_range,
            3_i64,
            libc::c_uint::MAX as libc::c_long,
            CLOSE_RANGE_CLOEXEC as libc::c_long,
        )
    };
    if ret != 0 {
        return Err(format!(
            "close_range(3, ~0, CLOEXEC) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
