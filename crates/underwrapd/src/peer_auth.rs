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

/// Check whether a peer is authorized based on UID/GID allowlists.
/// If both lists are empty, all peers are allowed.
pub(crate) fn check_peer_auth(
    peer: &Option<PeerCred>,
    allow_uids: &[u32],
    allow_gids: &[u32],
) -> std::result::Result<(), String> {
    if allow_uids.is_empty() && allow_gids.is_empty() {
        return Ok(());
    }

    let cred = match peer {
        Some(c) => c,
        None => return Err("could not determine peer credentials".to_string()),
    };

    if cred.uid == 0 {
        return Ok(());
    }

    if !allow_uids.is_empty() && allow_uids.contains(&cred.uid) {
        return Ok(());
    }

    if !allow_gids.is_empty() && allow_gids.contains(&cred.gid) {
        return Ok(());
    }

    Err(format!(
        "peer uid={} gid={} not in allowlist",
        cred.uid, cred.gid
    ))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_peer_auth_covers_allow_all_root_uid_uid_and_gid() {
        assert!(check_peer_auth(&None, &[], &[]).is_ok());

        let root = Some(PeerCred {
            pid: 1,
            uid: 0,
            gid: 999,
            egid: Some(999),
        });
        assert!(check_peer_auth(&root, &[1000], &[1001]).is_ok());

        let by_uid = Some(PeerCred {
            pid: 2,
            uid: 1000,
            gid: 2000,
            egid: Some(2000),
        });
        assert!(check_peer_auth(&by_uid, &[1000], &[]).is_ok());

        let by_gid = Some(PeerCred {
            pid: 3,
            uid: 3000,
            gid: 2000,
            egid: Some(2000),
        });
        assert!(check_peer_auth(&by_gid, &[], &[2000]).is_ok());

        let denied = check_peer_auth(&by_gid, &[1234], &[5678]).unwrap_err();
        assert!(denied.contains("not in allowlist"));
        assert!(check_peer_auth(&None, &[1234], &[]).is_err());
    }
}
