use super::*;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

const POINT: &str = "UNDERWRAP_FIXTURE_FAILURE_POINT";
const SOCKET: &str = "UNDERWRAP_FIXTURE_WITNESS_SOCKET";
const REAPS: &str = "UNDERWRAP_FIXTURE_WITNESS_REAPS";

fn assert_reaped(pid: u32, pidfd: &OwnedFd) {
    let mut fd = libc::pollfd {
        fd: pidfd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one live pollfd; waitpid uses a writable status slot for this
    // direct child. ECHILD after identity-bound exit establishes prior reap.
    assert_eq!(unsafe { libc::poll(&mut fd, 1, 0) }, 1);
    assert_ne!(fd.revents & libc::POLLIN, 0);
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

// Only the fresh selected test process receives these variables. No mutation
// of the parallel parent harness's environment or fixture identities is needed.
pub(crate) fn checkpoint(
    point: &str,
    state: &Arc<tempfile::TempDir>,
    children: &[&PackagedChild],
    endpoints: &[&str],
) -> bool {
    if std::env::var(POINT).ok().as_deref() != Some(point) {
        return false;
    }
    let mut stream = UnixStream::connect(std::env::var_os(SOCKET).unwrap()).unwrap();
    let message = serde_json::json!({
        "pids": children.iter().map(|child| child.id()).collect::<Vec<_>>(),
        "state": state.path(),
        "endpoints": endpoints,
    });
    serde_json::to_writer(&mut stream, &message).unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut ack = [0];
    stream.read_exact(&mut ack).unwrap();
    assert_eq!(ack, [1]);
    if point == "second-spawn-failure" {
        return true;
    }
    panic!("injected fixture failure: {point}");
}

pub(super) fn record_reap(pid: u32, state: &Path) {
    let Some(path) = std::env::var_os(REAPS) else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        // No panic or arbitrary child output in fallback diagnostics. This
        // marker is written only after Child::try_wait observed/reaped exit.
        let _ = writeln!(file, "{pid} {}", state.exists());
    }
}

#[test]
fn packaged_paths_reap_on_injected_failure() {
    let cases = [
        (
            "network_bootstrap_cannot_create_operator",
            "network-readiness",
        ),
        (
            "network_bootstrap_cannot_create_operator",
            "network-acquired",
        ),
        ("network_bootstrap_cannot_create_operator", "network-ready"),
        ("network_bootstrap_cannot_create_operator", "network-use"),
        (
            "bootstrap_user_processes_serialize_without_touching_other_state",
            "bootstrap-first",
        ),
        (
            "bootstrap_user_processes_serialize_without_touching_other_state",
            "bootstrap-second",
        ),
        (
            "bootstrap_lock_is_released_when_real_owner_is_killed",
            "owner-acquired",
        ),
        (
            "bootstrap_lock_is_released_when_real_owner_is_killed",
            "owner-use",
        ),
        ("underwrapd_binary_start_and_read", "provision-acquired"),
        ("underwrapd_binary_start_and_read", "backend-acquired"),
        ("underwrapd_binary_start_and_read", "backend-readiness"),
        ("underwrapd_binary_start_and_read", "backend-ready"),
        ("underwrapd_binary_start_and_read", "second-spawn-failure"),
        ("underwrapd_binary_start_and_read", "daemon-acquired"),
        ("underwrapd_binary_start_and_read", "daemon-readiness"),
        ("underwrapd_binary_start_and_read", "daemon-ready"),
        ("underwrapd_binary_start_and_read", "before-both-cleanup"),
    ];
    for (test, point) in cases {
        let witness = tempfile::TempDir::new().unwrap();
        let socket = witness.path().join("witness.sock");
        let reaps = witness.path().join("reaps");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut worker = crate::test_support::ExternalTest::spawn(
            &format!("tests::daemon::{test}"),
            &[
                (POINT, point.as_ref()),
                (SOCKET, socket.as_os_str()),
                (REAPS, reaps.as_os_str()),
            ],
        );
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    worker.check_running();
                    std::thread::sleep(POLL);
                }
                Err(error) => panic!("accepting fixture witness: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let message: serde_json::Value = serde_json::from_reader(&stream).unwrap();
        let pids: Vec<u32> = serde_json::from_value(message["pids"].clone()).unwrap();
        let state: std::path::PathBuf = serde_json::from_value(message["state"].clone()).unwrap();
        let endpoints: Vec<String> = serde_json::from_value(message["endpoints"].clone()).unwrap();
        assert!(state.exists());
        let pidfds: Vec<OwnedFd> = pids
            .iter()
            .map(|pid| {
                // SAFETY: pidfd_open takes no pointers. On success this call creates
                // one newly owned descriptor binding observation to this identity.
                let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, *pid, 0) };
                assert!(fd >= 0, "pidfd witness requires Linux pidfd_open");
                unsafe { OwnedFd::from_raw_fd(fd as i32) }
            })
            .collect();
        stream.write_all(&[1]).unwrap();
        assert!(
            !worker.wait().success(),
            "failure injection did not fail: {point}"
        );
        let observations = std::fs::read_to_string(&reaps).unwrap();
        for (pid, fd) in pids.iter().zip(&pidfds) {
            assert!(
                observations
                    .lines()
                    .any(|line| line == format!("{pid} true")),
                "missing owner-mediated reap with live state: {point}"
            );
            let mut pollfd = libc::pollfd {
                fd: fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one writable pollfd with a live identity-bound descriptor.
            assert_eq!(unsafe { libc::poll(&mut pollfd, 1, 0) }, 1);
            assert_ne!(pollfd.revents & libc::POLLIN, 0);
        }
        for endpoint in endpoints {
            if let Some(addr) = endpoint.strip_prefix("https://") {
                assert!(
                    std::net::TcpStream::connect(addr).is_err(),
                    "listener survived: {point}"
                );
            } else {
                assert!(
                    UnixStream::connect(endpoint).is_err(),
                    "socket survived: {point}"
                );
            }
        }
        assert!(
            !state.exists(),
            "state not released after confirmed reaps: {point}"
        );
    }
}

#[test]
fn output_wait_cancellation_keeps_owner_and_cleanup_uncertainty_keeps_state() {
    const NAME: &str = "harness::packaged_child::tests::output_wait_cancellation_keeps_owner_and_cleanup_uncertainty_keeps_state";
    const CHILD: &str = "UNDERWRAP_OUTPUT_WAIT_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let mut worker = crate::test_support::ExternalTest::spawn(NAME, &[(CHILD, "1".as_ref())]);
        assert!(
            worker.wait().success(),
            "isolated output/cleanup control failed"
        );
        return;
    }
    let state = Arc::new(tempfile::TempDir::new().unwrap());
    let path = state.path().to_path_buf();
    let config = underwrap_backend::BackendConfig {
        stores_dir: path.join("stores"),
        clients_dir: path.join("clients"),
        users_dir: path.join("users"),
        bootstrap_file: path.join("bootstrap.json"),
        https: None,
        admin: None,
    };
    let config_path = path.join("backend.yaml");
    std::fs::write(&config_path, serde_saphyr::to_string(&config).unwrap()).unwrap();
    let mut command = Command::new(
        std::env::var_os("CARGO_BIN_EXE_underwrap-backend").expect("packaged backend prerequisite"),
    );
    command
        .arg("--config")
        .arg(&config_path)
        .args(["admin", "bootstrap-user", "--username", "waiter"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut setup_pid = 0;
    let mut setup_pidfd = None;
    let setup_failure = PackagedChild::spawn_with(&mut command, &state, |owner| {
        setup_pid = owner.id();
        // SAFETY: scalar pidfd_open creates a new owned identity descriptor.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, setup_pid, 0) };
        assert!(fd >= 0);
        setup_pidfd = Some(unsafe { OwnedFd::from_raw_fd(fd as i32) });
        Err(io::Error::other(
            "injected setup error before output preparation",
        ))
    });
    assert!(setup_failure.is_err());
    assert_reaped(setup_pid, setup_pidfd.as_ref().unwrap());
    let mut child = PackagedChild::spawn(&mut command, &state).unwrap();
    let pid = child.id();
    // SAFETY: scalar pidfd_open creates a new owned identity descriptor.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    assert!(fd >= 0);
    let pidfd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        assert!(
            tokio::time::timeout(Duration::from_millis(30), child.wait_with_output())
                .await
                .is_err()
        );
    });
    assert!(child.try_wait().unwrap().is_none());
    // Inject an output-read failure at the same borrowed poll boundary.
    assert!(
        child
            .poll_output_with(|_| Err(io::Error::other("injected output failure")))
            .is_err()
    );
    assert!(child.try_wait().unwrap().is_none());
    drop(state);
    assert!(path.exists());
    child
        .kill_and_reap_with(
            |owner| {
                owner.kill()?;
                Err(io::Error::other("simulated signal error after delivery"))
            },
            PackagedChild::try_wait,
            CLEANUP_BUDGET,
        )
        .unwrap();
    assert!(child.status.is_some());
    assert_reaped(pid, &pidfd);
    let cleanup_error = child
        .kill_and_reap_with(
            |_| Err(io::Error::other("simulated signal error")),
            |_| Err(io::Error::other("simulated failed reap observation")),
            Duration::ZERO,
        )
        .unwrap_err();
    assert_eq!(cleanup_error.kind(), io::ErrorKind::TimedOut);
    // A simulated failed observation exercises Drop's retention path, but does
    // not intentionally orphan a real child. Preserve the original panic.
    child.cleanup_observation_error = true;
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _child = child;
        panic!("primary fixture failure");
    }));
    assert_eq!(
        failure.unwrap_err().downcast_ref::<&str>(),
        Some(&"primary fixture failure")
    );
    assert!(path.exists());
    // The uncertainty above was simulated, and independent pidfd/waitpid checks
    // already confirmed real reap. Release only this control's own scratch state.
    std::fs::remove_dir_all(path).unwrap();
}
