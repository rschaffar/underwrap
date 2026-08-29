//! Authentication for the privileged `underwrap-exec` → `underwrap-ns`
//! handoff.
//!
//! `underwrap-ns` is a system-wide capability wrapper, so argv, environment,
//! and fd 3 are untrusted until the process proves possession of the scoped
//! Unix socket that `underwrapd` created for this invocation. Linux records the
//! socketpair creator's credentials for `SO_PEERCRED`; passing the endpoint via
//! `SCM_RIGHTS`, `dup2`, fork, and exec does not change them.

use std::ffi::CString;

use underwrap_protocol::exec::WRAPPER_RESOURCE_CHANNEL_FD;

/// The root-provisioned daemon identity expected to have created the scoped
/// resource channel for one frontend instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DaemonIdentity {
    pub uid: libc::uid_t,
    pub gid: libc::gid_t,
    pub account: String,
}

/// Resolve the selected instance's daemon account and authenticate the fixed
/// fd-20 resource channel before any exec-bundle field or target selector is
/// trusted.
pub(crate) fn authenticate_instance_channel(instance: &str) -> Result<DaemonIdentity, String> {
    let identity = resolve_daemon_identity(instance)?;
    if identity.uid == 0 || identity.gid == 0 {
        return Err(format!(
            "selected instance daemon account {:?} resolves to privileged uid/gid {}/{}; refusing handoff",
            identity.account, identity.uid, identity.gid
        ));
    }
    authenticate_channel_fd(WRAPPER_RESOURCE_CHANNEL_FD, &identity)?;
    Ok(identity)
}

/// Reject a host configuration that collapses the trusted daemon principal and
/// target sandbox onto the same numeric identity.
pub(crate) fn require_distinct_target(
    daemon: &DaemonIdentity,
    target_uid: libc::uid_t,
    target_gid: libc::gid_t,
) -> Result<(), String> {
    if daemon.uid == target_uid || daemon.gid == target_gid {
        return Err(format!(
            "selected instance daemon identity {}/{} overlaps target identity {target_uid}/{target_gid}; refusing handoff",
            daemon.uid, daemon.gid
        ));
    }
    Ok(())
}

fn resolve_daemon_identity(instance: &str) -> Result<DaemonIdentity, String> {
    if instance.is_empty() {
        return Err("isolation instance is empty".to_string());
    }
    let account = format!("underwrap-{instance}");
    let c_account = CString::new(account.as_str())
        .map_err(|_| "isolation instance contains an interior NUL".to_string())?;

    // SAFETY: `c_account` is NUL-terminated. `getpwnam` returns static storage
    // or null; copy the numeric fields before any subsequent NSS call.
    let pwd = unsafe { libc::getpwnam(c_account.as_ptr()) };
    if pwd.is_null() {
        return Err(format!(
            "selected instance daemon account {account:?} does not exist"
        ));
    }
    Ok(DaemonIdentity {
        // SAFETY: `pwd` is non-null and its static NSS storage has not been
        // replaced; copy the initialized UID field before any later lookup.
        uid: unsafe { (*pwd).pw_uid },
        // SAFETY: the same checked pointer remains valid while the initialized
        // GID field is copied immediately in this single-threaded startup phase.
        gid: unsafe { (*pwd).pw_gid },
        account,
    })
}

fn socket_i32_option(fd: i32, option: i32, name: &str) -> Result<i32, String> {
    let mut value: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: `value` and `len` point to writable objects of the advertised
    // sizes; getsockopt writes at most `len` bytes.
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            option,
            (&mut value as *mut libc::c_int).cast(),
            &mut len,
        )
    };
    if ret != 0 {
        return Err(format!(
            "reading {name} from resource channel fd {fd}: {}",
            std::io::Error::last_os_error()
        ));
    }
    if len != std::mem::size_of::<libc::c_int>() as libc::socklen_t {
        return Err(format!(
            "resource channel fd {fd} returned malformed {name} length {len}"
        ));
    }
    Ok(value)
}

fn peer_credentials(fd: i32) -> Result<libc::ucred, String> {
    // SAFETY: all-zero is a valid initialized `ucred` value and provides aligned,
    // writable output storage; exact kernel output length is checked before use.
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` point to writable objects of the advertised
    // sizes; getsockopt writes at most `len` bytes.
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if ret != 0 {
        return Err(format!(
            "reading SO_PEERCRED from resource channel fd {fd}: {}",
            std::io::Error::last_os_error()
        ));
    }
    if len != std::mem::size_of::<libc::ucred>() as libc::socklen_t {
        return Err(format!(
            "resource channel fd {fd} returned malformed SO_PEERCRED length {len}"
        ));
    }
    if cred.pid <= 0 {
        return Err(format!(
            "resource channel fd {fd} returned invalid peer pid {}",
            cred.pid
        ));
    }
    Ok(cred)
}

fn authenticate_channel_fd(fd: i32, expected: &DaemonIdentity) -> Result<(), String> {
    let domain = socket_i32_option(fd, libc::SO_DOMAIN, "SO_DOMAIN")?;
    if domain != libc::AF_UNIX {
        return Err(format!(
            "resource channel fd {fd} has socket domain {domain}, expected AF_UNIX"
        ));
    }
    let socket_type = socket_i32_option(fd, libc::SO_TYPE, "SO_TYPE")?;
    if socket_type != libc::SOCK_STREAM {
        return Err(format!(
            "resource channel fd {fd} has socket type {socket_type}, expected SOCK_STREAM"
        ));
    }

    let cred = peer_credentials(fd)?;
    if cred.uid != expected.uid || cred.gid != expected.gid {
        return Err(format!(
            "resource channel fd {fd} peer uid/gid {}/{} does not match selected instance daemon {:?} ({}/{})",
            cred.uid, cred.gid, expected.account, expected.uid, expected.gid
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
