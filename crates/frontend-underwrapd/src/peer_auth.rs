use tokio::net::UnixStream;
use tracing::warn;

/// Peer credentials from SO_PEERCRED plus effective GID from /proc.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used via Debug formatting in tracing.
pub(crate) struct PeerCred {
    pub(crate) pid: u32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    /// Effective GID, read from /proc/<pid>/status.
    /// This is the one set by setgid wrappers.
    /// SO_PEERCRED.gid only reports the *real* GID.
    pub(crate) egid: Option<u32>,
}

pub(crate) fn get_peer_cred(stream: &UnixStream) -> Option<PeerCred> {
    use std::os::unix::io::AsRawFd;

    let std_fd = stream.as_raw_fd();
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            std_fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if ret == 0 {
        let egid = read_proc_egid(cred.pid as u32);
        Some(PeerCred {
            pid: cred.pid as u32,
            uid: cred.uid,
            gid: cred.gid,
            egid,
        })
    } else {
        warn!("failed to get SO_PEERCRED");
        None
    }
}

/// Read the effective GID from /proc/<pid>/status.
/// Returns None if the file can't be read (e.g. process already exited).
pub(crate) fn read_proc_egid(pid: u32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Gid:\t") {
            let fields: Vec<&str> = rest.split('\t').collect();
            if fields.len() >= 2 {
                return fields[1].parse().ok();
            }
        }
    }
    None
}
