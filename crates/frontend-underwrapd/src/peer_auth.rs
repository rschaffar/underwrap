use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::net::UnixStream;
use tracing::{info, warn};

/// How the peer's effective GID was resolved.
///
/// This is recorded on every accepted connection and surfaced in the
/// rejection audit line so a post-incident review can tell which
/// integrity property the decision rested on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeerAuthPath {
    /// Race-free: a `SO_PEERPIDFD` pidfd pinned the peer PID for the
    /// lifetime of the `/proc/<pid>/status` read, so the read cannot land
    /// on a recycled process. Requires Linux 6.5+.
    PeerPidfd,
    /// `SO_PEERCRED` PID plus a separate `/proc/<pid>/status` read. The
    /// PID could in principle be recycled between the two syscalls. Only
    /// used on pre-6.5 kernels where `SO_PEERPIDFD` is unavailable.
    PeerCredFallback,
}

/// Peer credentials: `SO_PEERCRED` UID/GID (real columns on this
/// platform) plus the effective GID read from `/proc`. The effective GID
/// is the one a setgid wrapper transition sets, and is the kernel-rooted
/// factor the daemon authorises against.
#[derive(Debug, Clone)]
#[allow(dead_code)] // pid / auth_path are consumed via Debug formatting in tracing.
pub(crate) struct PeerCred {
    pub(crate) pid: u32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) egid: Option<u32>,
    pub(crate) auth_path: PeerAuthPath,
}

/// Real and effective UID/GID parsed from `/proc/<pid>/status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProcIds {
    ruid: u32,
    euid: u32,
    rgid: u32,
    egid: u32,
}

pub(crate) fn get_peer_cred(stream: &UnixStream) -> Option<PeerCred> {
    use std::os::unix::io::AsRawFd;
    get_peer_cred_for_fd(stream.as_raw_fd())
}

fn get_peer_cred_for_fd(fd: RawFd) -> Option<PeerCred> {
    let cred = match peer_ucred(fd) {
        Some(c) => c,
        None => {
            warn!("failed to get SO_PEERCRED");
            return None;
        }
    };

    // Primary path: SO_PEERPIDFD. Holding the pidfd open pins the peer's
    // PID — the kernel will not recycle it to a different process — so the
    // subsequent `/proc/<pid>/status` read is race-free.
    if let Some(pidfd) = peer_pidfd(fd) {
        let resolved = (|| {
            let pid = pidfd_pid(pidfd)?;
            let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
            Some((pid, status))
        })();
        // Release the pidfd only after the read above has completed.
        unsafe { libc::close(pidfd) };

        return match resolved {
            Some((pid, status)) => finish_peer_cred(cred, pid, &status, PeerAuthPath::PeerPidfd),
            None => {
                // The pidfd resolved but the process vanished before we
                // could read it. Reject rather than silently dropping to
                // the racy fallback for a single connection.
                warn!("SO_PEERPIDFD obtained but peer /proc read failed; rejecting");
                None
            }
        };
    }

    // Fallback path: pre-6.5 kernel without SO_PEERPIDFD.
    warn_fallback_once();
    let pid = cred.pid as u32;
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    finish_peer_cred(cred, pid, &status, PeerAuthPath::PeerCredFallback)
}

/// Combine the kernel-supplied `SO_PEERCRED` with the parsed `/proc` ids,
/// rejecting the connection if the two disagree (the signature of PID
/// reuse on the fallback path).
fn finish_peer_cred(
    cred: libc::ucred,
    pid: u32,
    status: &str,
    auth_path: PeerAuthPath,
) -> Option<PeerCred> {
    let ids = parse_proc_status_ids(status)?;
    if !creds_consistent(cred.uid, cred.gid, &ids) {
        warn!(
            ?auth_path,
            cred_uid = cred.uid,
            cred_gid = cred.gid,
            proc_ruid = ids.ruid,
            proc_euid = ids.euid,
            proc_rgid = ids.rgid,
            proc_egid = ids.egid,
            "SO_PEERCRED uid/gid disagree with /proc/<pid>/status — possible PID reuse; rejecting peer",
        );
        return None;
    }
    Some(PeerCred {
        pid,
        uid: cred.uid,
        gid: cred.gid,
        egid: Some(ids.egid),
        auth_path,
    })
}

/// `SO_PEERCRED` (real UID/GID + PID, captured by the kernel at connect).
fn peer_ucred(fd: RawFd) -> Option<libc::ucred> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    (ret == 0).then_some(cred)
}

/// `SO_PEERPIDFD` (Linux 6.5+). Returns an owned pidfd the caller must
/// close, or `None` when the kernel doesn't support the option.
fn peer_pidfd(fd: RawFd) -> Option<RawFd> {
    let mut pidfd: libc::c_int = -1;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERPIDFD,
            &mut pidfd as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if ret == 0 && pidfd >= 0 {
        Some(pidfd)
    } else {
        None
    }
}

/// Resolve the PID a pidfd refers to via `/proc/self/fdinfo/<pidfd>`,
/// which exposes a stable `Pid:` line for as long as the pidfd is open.
fn pidfd_pid(pidfd: RawFd) -> Option<u32> {
    let fdinfo = std::fs::read_to_string(format!("/proc/self/fdinfo/{pidfd}")).ok()?;
    parse_fdinfo_pid(&fdinfo)
}

fn parse_fdinfo_pid(fdinfo: &str) -> Option<u32> {
    for line in fdinfo.lines() {
        if let Some(rest) = line.strip_prefix("Pid:") {
            // A pidfd whose process has been reaped reports `Pid: -1`.
            if let Ok(v) = rest.trim().parse::<i64>()
                && v >= 0
            {
                return u32::try_from(v).ok();
            }
            return None;
        }
    }
    None
}

/// Parse the real/effective UID and GID from `/proc/<pid>/status`.
/// The relevant lines look like `Uid:\t<real>\t<eff>\t<saved>\t<fs>`.
fn parse_proc_status_ids(status: &str) -> Option<ProcIds> {
    let mut uid = None;
    let mut gid = None;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            uid = parse_real_eff(rest);
        } else if let Some(rest) = line.strip_prefix("Gid:") {
            gid = parse_real_eff(rest);
        }
    }
    let (ruid, euid) = uid?;
    let (rgid, egid) = gid?;
    Some(ProcIds {
        ruid,
        euid,
        rgid,
        egid,
    })
}

fn parse_real_eff(rest: &str) -> Option<(u32, u32)> {
    let mut it = rest.split_whitespace();
    let real = it.next()?.parse().ok()?;
    let eff = it.next()?.parse().ok()?;
    Some((real, eff))
}

/// `SO_PEERCRED` reports the real UID/GID on this platform, but to stay
/// robust against the kernel ever reporting effective instead, accept a
/// match against either `/proc` column. A recycled PID would describe a
/// different process whose ids differ from the connect-time snapshot, so
/// the check still catches reuse on the fallback path.
fn creds_consistent(cred_uid: u32, cred_gid: u32, ids: &ProcIds) -> bool {
    (cred_uid == ids.ruid || cred_uid == ids.euid) && (cred_gid == ids.rgid || cred_gid == ids.egid)
}

static FALLBACK_WARNED: AtomicBool = AtomicBool::new(false);

fn warn_fallback_once() {
    if !FALLBACK_WARNED.swap(true, Ordering::Relaxed) {
        warn!(
            "wrapper peer auth fell back to SO_PEERCRED + /proc (kernel lacks SO_PEERPIDFD, \
             pre-6.5); this path is theoretically vulnerable to PID recycling. The \
             effective-GID socket-group gate still holds.",
        );
    }
}

/// Log, once at daemon startup, which peer-auth path this kernel will
/// use. Operators should know if they are on the weaker fallback.
pub(crate) fn log_peer_auth_mode() {
    if peerpidfd_supported() {
        info!("wrapper peer auth: SO_PEERPIDFD available (race-free PID pinning, Linux 6.5+)");
    } else {
        warn!(
            "wrapper peer auth: SO_PEERPIDFD unavailable (kernel < 6.5); using the \
             SO_PEERCRED + /proc fallback",
        );
    }
}

fn peerpidfd_supported() -> bool {
    let mut fds = [0 as libc::c_int; 2];
    let ret = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
    if ret != 0 {
        return false;
    }
    let supported = match peer_pidfd(fds[0]) {
        Some(pidfd) => {
            unsafe { libc::close(pidfd) };
            true
        }
        None => false,
    };
    unsafe {
        libc::close(fds[0]);
        libc::close(fds[1]);
    }
    supported
}

#[cfg(test)]
mod tests;
