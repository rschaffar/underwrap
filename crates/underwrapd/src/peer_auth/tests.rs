use std::cell::RefCell;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;

use super::*;

const STATUS: &str =
    "Name:\tgh\nState:\tR (running)\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\t6001\t6001\t6001\n";
const SOCKET_FD: RawFd = 71;
const PIDFD: RawFd = 72;
const PEER_PID: u32 = 4242;
const PIDFD_PID: u32 = 4343;

#[derive(Clone, Copy)]
enum Acquisition {
    Unsupported,
    Error(i32),
    InvalidData,
    Acquired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    Acquire(RawFd),
    ResolvePid(RawFd),
    ReadStatus(u32),
    Close(RawFd),
}

struct FakePeerAuthOperations {
    acquisition: Acquisition,
    resolved_pid: Option<u32>,
    status: Option<&'static str>,
    operations: RefCell<Vec<Operation>>,
}

impl FakePeerAuthOperations {
    fn new(
        acquisition: Acquisition,
        resolved_pid: Option<u32>,
        status: Option<&'static str>,
    ) -> Self {
        Self {
            acquisition,
            resolved_pid,
            status,
            operations: RefCell::new(Vec::new()),
        }
    }

    fn operations(&self) -> Vec<Operation> {
        self.operations.borrow().clone()
    }
}

impl PeerAuthOperations for FakePeerAuthOperations {
    type Pidfd = RawFd;

    fn peer_pidfd(&self, socket_fd: RawFd) -> std::io::Result<Option<Self::Pidfd>> {
        self.operations
            .borrow_mut()
            .push(Operation::Acquire(socket_fd));
        match self.acquisition {
            Acquisition::Unsupported => Ok(None),
            Acquisition::Error(errno) => Err(std::io::Error::from_raw_os_error(errno)),
            Acquisition::InvalidData => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "malformed descriptor payload",
            )),
            Acquisition::Acquired => Ok(Some(PIDFD)),
        }
    }

    fn pidfd_pid(&self, pidfd: &Self::Pidfd) -> Option<u32> {
        self.operations
            .borrow_mut()
            .push(Operation::ResolvePid(*pidfd));
        self.resolved_pid
    }

    fn read_proc_status(&self, pid: u32) -> Option<String> {
        self.operations
            .borrow_mut()
            .push(Operation::ReadStatus(pid));
        self.status.map(str::to_owned)
    }

    fn close_pidfd(&self, pidfd: Self::Pidfd) {
        self.operations.borrow_mut().push(Operation::Close(pidfd));
    }
}

fn socket_cred() -> libc::ucred {
    libc::ucred {
        pid: PEER_PID as libc::pid_t,
        uid: 1000,
        gid: 1000,
    }
}

fn resolve_with(operations: &FakePeerAuthOperations) -> Result<PeerCred, PeerAuthReject> {
    resolve_peer_cred(SOCKET_FD, socket_cred(), operations)
}

#[test]
fn parse_proc_status_ids_extracts_real_and_effective() {
    let ids = parse_proc_status_ids(STATUS).expect("must parse");
    assert_eq!(ids.ruid, 1000);
    assert_eq!(ids.euid, 1000);
    assert_eq!(ids.rgid, 1000);
    assert_eq!(ids.egid, 6001, "effective gid is the setgid wrapper group");
}

#[test]
fn parse_proc_status_ids_handles_space_separated() {
    // Defensive: some readers normalise tabs to spaces.
    let status = "Uid: 1000 1000 1000 1000\nGid: 1000 6001 6001 6001\n";
    let ids = parse_proc_status_ids(status).expect("must parse");
    assert_eq!(ids.rgid, 1000);
    assert_eq!(ids.egid, 6001);
}

#[test]
fn parse_proc_status_ids_rejects_missing_lines() {
    assert!(parse_proc_status_ids("Uid:\t1000\t1000\t1000\t1000\n").is_none());
    assert!(parse_proc_status_ids("Gid:\t1000\t6001\t6001\t6001\n").is_none());
}

#[test]
fn creds_consistent_matches_real_column() {
    let ids = ProcIds {
        ruid: 1000,
        euid: 1000,
        rgid: 1000,
        egid: 6001,
    };
    // SO_PEERCRED reports the real uid/gid here.
    assert!(creds_consistent(1000, 1000, &ids));
}

#[test]
fn creds_consistent_matches_effective_column() {
    // If a kernel ever reported effective via SO_PEERCRED, the egid must
    // still be accepted (robustness against the real-vs-effective question).
    let ids = ProcIds {
        ruid: 1000,
        euid: 1000,
        rgid: 1000,
        egid: 6001,
    };
    assert!(creds_consistent(1000, 6001, &ids));
}

#[test]
fn creds_consistent_rejects_unrelated_ids() {
    // The fingerprint of PID reuse: /proc now describes a different
    // process whose ids match neither SO_PEERCRED column.
    let ids = ProcIds {
        ruid: 4242,
        euid: 4242,
        rgid: 4242,
        egid: 4242,
    };
    assert!(!creds_consistent(1000, 1000, &ids));
}

#[test]
fn finish_peer_cred_reports_cred_mismatch() {
    // /proc describes a process whose ids match neither SO_PEERCRED
    // column — the PID-reuse signature. The reject must carry the
    // distinct CredMismatch arm (so intake can audit it), not Unresolved,
    // and surface the SO_PEERCRED uid/gid plus the auth path.
    let cred = libc::ucred {
        pid: 31337,
        uid: 1000,
        gid: 1000,
    };
    let status = "Uid:\t4242\t4242\t4242\t4242\nGid:\t4242\t4242\t4242\t4242\n";
    let err = finish_peer_cred(cred, 31337, status, PeerAuthPath::PeerCredFallback)
        .expect_err("mismatched ids must be rejected");
    assert_eq!(
        err,
        PeerAuthReject::CredMismatch {
            uid: 1000,
            gid: 1000,
            auth_path: PeerAuthPath::PeerCredFallback,
        },
    );
}

#[test]
fn finish_peer_cred_reports_unresolved_on_unparseable_status() {
    // A /proc/<pid>/status we cannot parse yields nothing reliable about
    // the peer, so it is Unresolved rather than a mismatch.
    let cred = libc::ucred {
        pid: 31337,
        uid: 1000,
        gid: 1000,
    };
    let err = finish_peer_cred(cred, 31337, "garbage\n", PeerAuthPath::PeerPidfd)
        .expect_err("unparseable status must be rejected");
    assert_eq!(err, PeerAuthReject::Unresolved);
}

#[test]
fn finish_peer_cred_accepts_consistent_ids() {
    let cred = libc::ucred {
        pid: 31337,
        uid: 1000,
        gid: 1000,
    };
    let peer = finish_peer_cred(cred, 31337, STATUS, PeerAuthPath::PeerPidfd)
        .expect("consistent ids must be accepted");
    assert_eq!(peer.uid, 1000);
    assert_eq!(peer.egid, Some(6001));
    assert_eq!(peer.auth_path, PeerAuthPath::PeerPidfd);
}

#[test]
fn parse_fdinfo_pid_extracts_stable_pid() {
    let fdinfo = "pos:\t0\nflags:\t02000000\nmnt_id:\t15\nPid:\t31337\nNSpid:\t31337\n";
    assert_eq!(parse_fdinfo_pid(fdinfo), Some(31337));
}

#[test]
fn parse_fdinfo_pid_rejects_reaped_process() {
    // A pidfd whose target has been reaped reports `Pid: -1`.
    let fdinfo = "pos:\t0\nflags:\t02000000\nPid:\t-1\nNSpid:\t-1\n";
    assert_eq!(parse_fdinfo_pid(fdinfo), None);
}

#[test]
fn parse_fdinfo_pid_rejects_missing_pid_line() {
    // An unexpected procfs shape with no `Pid:` line must fail closed:
    // the caller rejects the connection rather than widening the gate.
    let fdinfo = "pos:\t0\nflags:\t02000000\nmnt_id:\t15\n";
    assert_eq!(parse_fdinfo_pid(fdinfo), None);
}

#[test]
fn only_enoprotoopt_is_classified_as_peer_pidfd_unsupported() {
    let unsupported =
        classify_peer_pidfd_error(std::io::Error::from_raw_os_error(libc::ENOPROTOOPT))
            .expect("ENOPROTOOPT selects the explicitly weaker older-kernel path");
    assert_eq!(unsupported, None);

    for errno in [
        libc::EACCES,
        libc::EBADF,
        libc::EINTR,
        libc::EINVAL,
        libc::EMFILE,
    ] {
        let err = classify_peer_pidfd_error(std::io::Error::from_raw_os_error(errno))
            .expect_err("every non-ENOPROTOOPT errno must remain an acquisition failure");
        assert_eq!(err.raw_os_error(), Some(errno));
    }
}

#[test]
fn valid_and_negative_peer_pidfd_payloads_transfer_only_valid_ownership() {
    let expected_len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    let (pidfd, _peer) = UnixStream::pair().expect("temporary Unix descriptors must be created");
    let pidfd = pidfd.into_raw_fd();
    let returned = validate_peer_pidfd_payload(pidfd, expected_len)
        .expect("a valid descriptor payload must be returned");
    assert_eq!(returned, pidfd);

    // SAFETY: validation returned the same owned descriptor, transferring it
    // back to this test exactly once.
    let returned = unsafe { OwnedFd::from_raw_fd(returned) };
    assert_eq!(returned.as_raw_fd(), pidfd);

    assert_eq!(
        validate_peer_pidfd_payload(-1, expected_len)
            .expect_err("a negative descriptor must be rejected")
            .kind(),
        std::io::ErrorKind::InvalidData,
    );
}

#[test]
fn malformed_peer_pidfd_length_closes_the_acquired_descriptor() {
    let expected_len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    let (pidfd, _peer) = UnixStream::pair().expect("temporary Unix descriptors must be created");
    let pidfd = pidfd.into_raw_fd();

    assert_eq!(
        validate_peer_pidfd_payload(pidfd, expected_len - 1)
            .expect_err("a malformed descriptor length must be rejected")
            .kind(),
        std::io::ErrorKind::InvalidData,
    );
    let fcntl_result = unsafe { libc::fcntl(pidfd, libc::F_GETFD) };
    let fcntl_error = std::io::Error::last_os_error();
    assert_eq!(fcntl_result, -1);
    assert_eq!(fcntl_error.raw_os_error(), Some(libc::EBADF));
}

#[test]
fn unsupported_peer_pidfd_alone_reads_fallback_status() {
    let operations = FakePeerAuthOperations::new(Acquisition::Unsupported, None, Some(STATUS));
    let peer = resolve_with(&operations).expect("unsupported kernels retain the explicit fallback");

    assert_eq!(peer.pid, PEER_PID);
    assert_eq!(peer.auth_path, PeerAuthPath::PeerCredFallback);
    assert_eq!(
        operations.operations(),
        [
            Operation::Acquire(SOCKET_FD),
            Operation::ReadStatus(PEER_PID)
        ]
    );
}

#[test]
fn peer_pidfd_acquisition_errors_reject_without_fallback_status_reads() {
    for acquisition in [
        Acquisition::Error(libc::EACCES),
        Acquisition::Error(libc::EBADF),
        Acquisition::Error(libc::EINTR),
        Acquisition::InvalidData,
    ] {
        let operations = FakePeerAuthOperations::new(acquisition, None, Some(STATUS));
        assert_eq!(
            resolve_with(&operations).expect_err("acquisition failure must reject"),
            PeerAuthReject::Unresolved,
        );
        assert_eq!(operations.operations(), [Operation::Acquire(SOCKET_FD)]);
    }
}

#[test]
fn acquired_peer_pidfd_pid_resolution_failure_rejects_and_closes_once() {
    let operations = FakePeerAuthOperations::new(Acquisition::Acquired, None, Some(STATUS));
    assert_eq!(
        resolve_with(&operations).expect_err("PID resolution failure must reject"),
        PeerAuthReject::Unresolved,
    );
    assert_eq!(
        operations.operations(),
        [
            Operation::Acquire(SOCKET_FD),
            Operation::ResolvePid(PIDFD),
            Operation::Close(PIDFD),
        ]
    );
}

#[test]
fn acquired_peer_pidfd_status_read_failure_rejects_without_fallback_and_closes_once() {
    let operations = FakePeerAuthOperations::new(Acquisition::Acquired, Some(PIDFD_PID), None);
    assert_eq!(
        resolve_with(&operations).expect_err("status read failure must reject"),
        PeerAuthReject::Unresolved,
    );
    assert_eq!(
        operations.operations(),
        [
            Operation::Acquire(SOCKET_FD),
            Operation::ResolvePid(PIDFD),
            Operation::ReadStatus(PIDFD_PID),
            Operation::Close(PIDFD),
        ]
    );
}

#[test]
fn acquired_peer_pidfd_malformed_status_rejects_without_fallback_and_closes_once() {
    let operations =
        FakePeerAuthOperations::new(Acquisition::Acquired, Some(PIDFD_PID), Some("garbage\n"));
    assert_eq!(
        resolve_with(&operations).expect_err("malformed status must reject"),
        PeerAuthReject::Unresolved,
    );
    assert_eq!(
        operations.operations(),
        [
            Operation::Acquire(SOCKET_FD),
            Operation::ResolvePid(PIDFD),
            Operation::ReadStatus(PIDFD_PID),
            Operation::Close(PIDFD),
        ]
    );
}

#[test]
fn acquired_peer_pidfd_credential_mismatch_rejects_without_fallback_and_closes_once() {
    let mismatch = "Uid:\t4242\t4242\t4242\t4242\nGid:\t4242\t4242\t4242\t4242\n";
    let operations =
        FakePeerAuthOperations::new(Acquisition::Acquired, Some(PIDFD_PID), Some(mismatch));
    assert_eq!(
        resolve_with(&operations).expect_err("credential mismatch must reject"),
        PeerAuthReject::CredMismatch {
            uid: 1000,
            gid: 1000,
            auth_path: PeerAuthPath::PeerPidfd,
        }
    );
    assert_eq!(
        operations.operations(),
        [
            Operation::Acquire(SOCKET_FD),
            Operation::ResolvePid(PIDFD),
            Operation::ReadStatus(PIDFD_PID),
            Operation::Close(PIDFD),
        ]
    );
}

#[test]
fn acquired_peer_pidfd_success_uses_pinned_path_and_closes_once_after_status_read() {
    let operations =
        FakePeerAuthOperations::new(Acquisition::Acquired, Some(PIDFD_PID), Some(STATUS));
    let peer = resolve_with(&operations).expect("consistent pinned credentials must pass");

    assert_eq!(peer.pid, PIDFD_PID);
    assert_eq!(peer.egid, Some(6001));
    assert_eq!(peer.auth_path, PeerAuthPath::PeerPidfd);
    assert_eq!(
        operations.operations(),
        [
            Operation::Acquire(SOCKET_FD),
            Operation::ResolvePid(PIDFD),
            Operation::ReadStatus(PIDFD_PID),
            Operation::Close(PIDFD),
        ]
    );
}

#[test]
fn get_peer_cred_resolves_self_over_socketpair() {
    // End-to-end against a real socket: supported kernels must take the pinned
    // path. An older kernel remains portable only through the documented,
    // explicitly weaker fallback contract.
    let supported = peerpidfd_supported().expect("SO_PEERPIDFD support probe must complete");
    let mut fds = [0 as libc::c_int; 2];
    let ret = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
    assert_eq!(ret, 0, "socketpair failed");

    let cred = get_peer_cred_for_fd(fds[0]);
    unsafe {
        libc::close(fds[0]);
        libc::close(fds[1]);
    }

    let cred = cred.expect("peer of a socketpair is authenticated");
    assert_eq!(cred.pid, std::process::id());
    assert_eq!(cred.uid, unsafe { libc::getuid() });
    assert_eq!(cred.egid, Some(unsafe { libc::getegid() }));
    assert_eq!(
        cred.auth_path,
        if supported {
            PeerAuthPath::PeerPidfd
        } else {
            PeerAuthPath::PeerCredFallback
        },
    );
}

#[test]
fn peer_pidfd_preserves_unexpected_getsockopt_errors() {
    let err = peer_pidfd(-1).expect_err("an invalid socket fd must not select the fallback");
    assert_eq!(err.raw_os_error(), Some(libc::EBADF));
}

#[test]
fn log_peer_auth_mode_does_not_panic() {
    // Smoke: the startup probe must run on any kernel without crashing.
    log_peer_auth_mode();
}
