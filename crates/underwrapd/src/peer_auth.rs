use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd, RawFd};
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

/// Why a peer connection could not be authenticated. Distinguishing
/// these lets the intake layer emit a forensically useful audit event:
/// a cross-check failure is the signature of PID reuse and must be at
/// least as visible as an ordinary egid mismatch, whereas an unresolved
/// peer carries no reliable identity to log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeerAuthReject {
    /// The peer's credentials could not be resolved at all (no
    /// `SO_PEERCRED`, no readable/parseable `/proc`). Nothing reliable is
    /// known about the peer.
    Unresolved,
    /// `SO_PEERCRED` uid/gid disagreed with `/proc/<pid>/status` — the
    /// signature of PID reuse. The uid/gid are the `SO_PEERCRED` values
    /// captured at connect time.
    CredMismatch {
        uid: u32,
        gid: u32,
        auth_path: PeerAuthPath,
    },
}

/// Real and effective UID/GID parsed from `/proc/<pid>/status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProcIds {
    ruid: u32,
    euid: u32,
    rgid: u32,
    egid: u32,
}

pub(crate) fn get_peer_cred(stream: &UnixStream) -> Result<PeerCred, PeerAuthReject> {
    use std::os::unix::io::AsRawFd;
    get_peer_cred_for_fd(stream.as_raw_fd())
}

fn get_peer_cred_for_fd(fd: RawFd) -> Result<PeerCred, PeerAuthReject> {
    let cred = match peer_ucred(fd) {
        Some(c) => c,
        None => {
            warn!("failed to get SO_PEERCRED");
            return Err(PeerAuthReject::Unresolved);
        }
    };
    resolve_peer_cred(fd, cred, &SystemPeerAuthOperations)
}

/// Operations at the peer-pidfd decision boundary. Production delegates each
/// method directly to the existing syscall/procfs operation; tests inject only
/// here so they exercise the same fallback, rejection, and close ordering as a
/// live connection without adding a deployed fault-control path.
trait PeerAuthOperations {
    type Pidfd;

    fn peer_pidfd(&self, socket_fd: RawFd) -> std::io::Result<Option<Self::Pidfd>>;
    fn pidfd_pid(&self, pidfd: &Self::Pidfd) -> Option<u32>;
    fn read_proc_status(&self, pid: u32) -> Option<String>;
    fn close_pidfd(&self, pidfd: Self::Pidfd);
}

struct SystemPeerAuthOperations;

impl PeerAuthOperations for SystemPeerAuthOperations {
    type Pidfd = RawFd;

    fn peer_pidfd(&self, socket_fd: RawFd) -> std::io::Result<Option<Self::Pidfd>> {
        peer_pidfd(socket_fd)
    }

    fn pidfd_pid(&self, pidfd: &Self::Pidfd) -> Option<u32> {
        pidfd_pid(*pidfd)
    }

    fn read_proc_status(&self, pid: u32) -> Option<String> {
        std::fs::read_to_string(format!("/proc/{pid}/status")).ok()
    }

    fn close_pidfd(&self, pidfd: Self::Pidfd) {
        // SAFETY: `peer_pidfd` returned this descriptor as owned. The guard
        // below transfers it here exactly once and no later operation uses it.
        unsafe { libc::close(pidfd) };
    }
}

/// Owns one successfully acquired pidfd until every resolution operation has
/// finished. Keeping close in `Drop` makes each early rejection close exactly
/// once while the operation trace remains injectable in deterministic tests.
struct AcquiredPidfd<'a, O: PeerAuthOperations + ?Sized> {
    operations: &'a O,
    pidfd: Option<O::Pidfd>,
}

impl<'a, O: PeerAuthOperations + ?Sized> AcquiredPidfd<'a, O> {
    fn new(operations: &'a O, pidfd: O::Pidfd) -> Self {
        Self {
            operations,
            pidfd: Some(pidfd),
        }
    }

    fn get(&self) -> &O::Pidfd {
        self.pidfd
            .as_ref()
            .expect("acquired pidfd remains owned until drop")
    }
}

impl<O: PeerAuthOperations + ?Sized> Drop for AcquiredPidfd<'_, O> {
    fn drop(&mut self) {
        if let Some(pidfd) = self.pidfd.take() {
            self.operations.close_pidfd(pidfd);
        }
    }
}

fn resolve_peer_cred<O: PeerAuthOperations + ?Sized>(
    fd: RawFd,
    cred: libc::ucred,
    operations: &O,
) -> Result<PeerCred, PeerAuthReject> {
    // Primary path: SO_PEERPIDFD. Holding the pidfd open pins the peer's
    // PID — the kernel will not recycle it to a different process — so the
    // subsequent `/proc/<pid>/status` read is race-free. Only ENOPROTOOPT
    // selects the older-kernel fallback; every other acquisition failure is a
    // hard rejection.
    match operations.peer_pidfd(fd) {
        Ok(Some(raw_pidfd)) => {
            let pidfd = AcquiredPidfd::new(operations, raw_pidfd);
            let resolved = operations
                .pidfd_pid(pidfd.get())
                .and_then(|pid| operations.read_proc_status(pid).map(|status| (pid, status)));
            // Release the pidfd only after its PID and status read have
            // completed, and before parsing data that no longer needs the pin.
            drop(pidfd);

            return match resolved {
                Some((pid, status)) => {
                    finish_peer_cred(cred, pid, &status, PeerAuthPath::PeerPidfd)
                }
                None => {
                    // Reached when either the pidfd's `/proc/self/fdinfo` had no
                    // parseable `Pid:` line (an unexpected procfs shape) or the
                    // peer's `/proc/<pid>/status` could not be read. Both are
                    // hard auth failures *by design*: we reject rather than drop
                    // to the racy SO_PEERCRED fallback for a single connection.
                    // `hardenTargetHost` pins the kernel floor (>= 6.5) on which
                    // pidfd fdinfo is guaranteed to expose `Pid:`, so the
                    // fdinfo-shape case is not expected on a supported host.
                    warn!("SO_PEERPIDFD obtained but peer PID/proc resolution failed; rejecting");
                    Err(PeerAuthReject::Unresolved)
                }
            };
        }
        Ok(None) => {}
        Err(err) => {
            warn!(%err, "SO_PEERPIDFD acquisition failed unexpectedly; rejecting peer");
            return Err(PeerAuthReject::Unresolved);
        }
    }

    // Fallback path: pre-6.5 kernel without SO_PEERPIDFD.
    warn_fallback_once();
    let pid = cred.pid as u32;
    let status = match operations.read_proc_status(pid) {
        Some(status) => status,
        None => return Err(PeerAuthReject::Unresolved),
    };
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
) -> Result<PeerCred, PeerAuthReject> {
    let ids = match parse_proc_status_ids(status) {
        Some(ids) => ids,
        None => return Err(PeerAuthReject::Unresolved),
    };
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
        return Err(PeerAuthReject::CredMismatch {
            uid: cred.uid,
            gid: cred.gid,
            auth_path,
        });
    }
    Ok(PeerCred {
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
/// close, `Ok(None)` only when the kernel reports that the option is unsupported,
/// and an error for every other failure.
fn peer_pidfd(fd: RawFd) -> std::io::Result<Option<RawFd>> {
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
    if ret != 0 {
        return classify_peer_pidfd_error(std::io::Error::last_os_error());
    }
    validate_peer_pidfd_payload(pidfd, len).map(Some)
}

fn classify_peer_pidfd_error(err: std::io::Error) -> std::io::Result<Option<RawFd>> {
    if err.raw_os_error() == Some(libc::ENOPROTOOPT) {
        Ok(None)
    } else {
        Err(err)
    }
}

fn validate_peer_pidfd_payload(pidfd: RawFd, len: libc::socklen_t) -> std::io::Result<RawFd> {
    if pidfd < 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "SO_PEERPIDFD returned an invalid descriptor payload",
        ));
    }

    // SAFETY: a successful SO_PEERPIDFD call returned this nonnegative
    // descriptor as owned. Take ownership before validating the returned length
    // so every malformed payload closes the acquired descriptor exactly once.
    let pidfd = unsafe { OwnedFd::from_raw_fd(pidfd) };
    if len != std::mem::size_of::<libc::c_int>() as libc::socklen_t {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "SO_PEERPIDFD returned an invalid descriptor payload",
        ))
    } else {
        Ok(pidfd.into_raw_fd())
    }
}

/// Resolve the PID a pidfd refers to via `/proc/self/fdinfo/<pidfd>`,
/// which exposes a stable `Pid:` line for as long as the pidfd is open.
fn pidfd_pid(pidfd: RawFd) -> Option<u32> {
    let fdinfo = std::fs::read_to_string(format!("/proc/self/fdinfo/{pidfd}")).ok()?;
    parse_fdinfo_pid(&fdinfo)
}

/// Returns `None` for any fdinfo we cannot interpret as a live PID: a
/// reaped pidfd (`Pid: -1`), or a missing/unparseable `Pid:` line. The
/// caller treats `None` as a hard auth failure rather than widening the
/// gate — fail-closed by design. Supported kernels (>= 6.5, enforced by
/// `hardenTargetHost`) always expose `Pid:`, so the missing-line case is
/// an unexpected-procfs guard, not a path taken in normal operation.
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
    match peerpidfd_supported() {
        Ok(true) => {
            info!("wrapper peer auth: SO_PEERPIDFD available (race-free PID pinning, Linux 6.5+)")
        }
        Ok(false) => warn!(
            "wrapper peer auth: SO_PEERPIDFD unavailable (kernel < 6.5); using the \
             SO_PEERCRED + /proc fallback",
        ),
        Err(err) => warn!(
            %err,
            "wrapper peer auth: SO_PEERPIDFD probe failed unexpectedly; peer connections will reject"
        ),
    }
}

fn peerpidfd_supported() -> std::io::Result<bool> {
    let mut fds = [0 as libc::c_int; 2];
    let ret = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let supported = peer_pidfd(fds[0]).map(|pidfd| {
        if let Some(pidfd) = pidfd {
            unsafe { libc::close(pidfd) };
            true
        } else {
            false
        }
    });
    unsafe {
        libc::close(fds[0]);
        libc::close(fds[1]);
    }
    supported
}

#[cfg(test)]
mod tests;
