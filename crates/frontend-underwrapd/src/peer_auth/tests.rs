use super::*;

const STATUS: &str =
    "Name:\tgh\nState:\tR (running)\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\t6001\t6001\t6001\n";

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
fn get_peer_cred_resolves_self_over_socketpair() {
    // End-to-end against a real socket: a socketpair's peer is this very
    // process, so the resolved ids must match our own. Exercises whichever
    // path the running kernel supports (pidfd on 6.5+, else fallback).
    let mut fds = [0 as libc::c_int; 2];
    let ret = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
    assert_eq!(ret, 0, "socketpair failed");

    let cred = get_peer_cred_for_fd(fds[0]);
    unsafe {
        libc::close(fds[0]);
        libc::close(fds[1]);
    }

    let cred = cred.expect("peer of a socketpair is resolvable");
    assert_eq!(cred.pid, std::process::id());
    assert_eq!(cred.uid, unsafe { libc::getuid() });
    assert_eq!(cred.egid, Some(unsafe { libc::getegid() }));
}

#[test]
fn log_peer_auth_mode_does_not_panic() {
    // Smoke: the startup probe must run on any kernel without crashing.
    log_peer_auth_mode();
}
