use super::*;
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn https_readiness_elapsed_stall_success_and_failed_backend_controls() {
    const NAME: &str = "harness::readiness::tests::https_readiness_elapsed_stall_success_and_failed_backend_controls";
    const CHILD: &str = "UNDERWRAP_READINESS_DEADLINE_CHILD";
    const METRICS: &str = "UNDERWRAP_READINESS_METRICS";
    if std::env::var_os(CHILD).is_none() {
        let state = tempfile::TempDir::new().unwrap();
        let metrics = state.path().join("elapsed.json");
        let mut worker = crate::test_support::ExternalTest::spawn(
            NAME,
            &[(CHILD, "1".as_ref()), (METRICS, metrics.as_os_str())],
        );
        assert!(
            worker.wait().success(),
            "isolated elapsed-readiness witness failed"
        );
        let elapsed: Vec<u64> = serde_json::from_slice(&std::fs::read(metrics).unwrap()).unwrap();
        assert_eq!(elapsed.len(), 5);
        println!(
            "elapsed microseconds [admin stall, runtime stall, admin success, runtime success, failed backend]: {elapsed:?}"
        );
        return;
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut timings = Vec::new();
    rt.block_on(async {
        let state = tempfile::TempDir::new().unwrap();
        let pki = crate::harness::pki::test_pki_paths(state.path(), "local").unwrap();
        for identity in [None, Some(pki.client_identity.as_path())] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let url = format!("https://{}", listener.local_addr().unwrap());
            let budget = Duration::from_millis(240);
            let started = Instant::now();
            let error = wait_for_https(&url, &pki.ca_cert, identity, budget, || Ok(()))
                .await
                .unwrap_err();
            let elapsed = started.elapsed();
            timings.push(elapsed.as_micros() as u64);
            assert!(
                elapsed >= budget,
                "readiness returned before its elapsed budget"
            );
            assert!(
                elapsed < Duration::from_secs(1),
                "remaining request budget was not enforced"
            );
            assert!(error.to_string().contains("within 240ms"));
            // Observe a real queued TCP connection; this is stalled TLS, not a
            // quick-refusal loop or a mocked timer.
            let (stream, _) = listener
                .accept()
                .expect("probe never connected to stalled listener");
            drop(stream);
            drop(listener);
            println!(
                "{} stalled probe elapsed: {elapsed:?}",
                if identity.is_some() {
                    "runtime"
                } else {
                    "admin"
                }
            );
        }

        let fixture = crate::harness::TestDaemon::start(std::collections::HashMap::new())
            .await
            .unwrap();
        for identity in [None, Some(fixture.service_client_identity.as_path())] {
            let started = Instant::now();
            wait_for_https(
                &fixture.service_url,
                &fixture.service_ca_cert,
                identity,
                Duration::from_secs(1),
                || Ok(()),
            )
            .await
            .unwrap();
            timings.push(started.elapsed().as_micros() as u64);
            assert!(started.elapsed() < Duration::from_secs(1));
        }
        fixture.stop().await.unwrap(); // normal awaited shutdown, never timeout a join

        let state = std::sync::Arc::new(tempfile::TempDir::new().unwrap());
        let pki = crate::harness::pki::test_pki_paths(state.path(), "local").unwrap();
        let reservation = reserve_backend_startup().await.unwrap();
        let url = format!("https://127.0.0.1:{}", reservation.port());
        let config = underwrap_backend::BackendConfig {
            stores_dir: state.path().join("stores"),
            clients_dir: state.path().join("clients"),
            users_dir: state.path().join("users"),
            bootstrap_file: state.path().join("bootstrap.json"),
            https: Some(underwrap_backend::HttpsConfig {
                listen: format!("127.0.0.1:{}", reservation.port()),
                tls: underwrap_backend::HttpsTlsConfig {
                    cert_file: state.path().join("missing-cert"),
                    key_file: state.path().join("missing-key"),
                },
            }),
            admin: None,
        };
        let shutdown = std::sync::Arc::new(tokio::sync::Notify::new());
        let thread_shutdown = shutdown.clone();
        let thread_state = state.clone();
        let handle = std::thread::spawn(move || {
            let _state = thread_state;
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let result = rt.block_on(underwrap_backend::server::run(
                config,
                Some(thread_shutdown),
            ));
            assert!(
                result.is_err(),
                "missing TLS files must fail backend startup"
            );
            result
        });
        let owner = crate::harness::BackendThreadOwner::new(shutdown, handle);
        let started = Instant::now();
        let error = wait_for_admin(&url, &pki.ca_cert, || owner.ensure_running())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("backend exited"));
        timings.push(started.elapsed().as_micros() as u64);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(
            backend_port_startup_lock().try_lock().is_err(),
            "readiness does not release its caller's reservation"
        );
        drop(owner); // synchronous, observed failed thread; not inside Tokio timeout
        drop(reservation);
        let _next = reserve_backend_startup().await.unwrap();
    });
    std::fs::write(
        std::env::var_os(METRICS).unwrap(),
        serde_json::to_vec(&timings).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn backend_startup_reservation_excludes_processes_until_drop() {
    const CHILD_ENV: &str = "UNDERWRAP_BACKEND_STARTUP_LOCK_CHILD";
    const TEST_NAME: &str =
        "harness::readiness::tests::backend_startup_reservation_excludes_processes_until_drop";

    if std::env::var(CHILD_ENV).ok().as_deref() == Some(TEST_NAME) {
        let lock_file = open_backend_startup_lock_file().unwrap();
        // SAFETY: lock_file owns a valid descriptor for this nonblocking lock
        // probe. The parent process deliberately retains the conflicting lock.
        let result = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        let error = std::io::Error::last_os_error();
        assert_eq!(result, -1, "the child unexpectedly acquired the lock");
        assert_eq!(
            error.raw_os_error(),
            Some(libc::EWOULDBLOCK),
            "unexpected lock error: {error}"
        );
        return;
    }

    // No unrelated fixture may take the just-released cross-process lock
    // between drop and the exact nonblocking release assertion below. Isolate
    // this witness's TMPDIR lock domain instead of weakening that assertion.
    const ISOLATED: &str = "UNDERWRAP_STARTUP_LOCK_ISOLATED";
    if std::env::var_os(ISOLATED).is_none() {
        let state = tempfile::TempDir::new().unwrap();
        let mut worker = crate::test_support::ExternalTest::spawn(
            TEST_NAME,
            &[
                (ISOLATED, "1".as_ref()),
                ("TMPDIR", state.path().as_os_str()),
            ],
        );
        assert!(
            worker.wait().success(),
            "isolated reservation witness failed"
        );
        return;
    }
    let reservation = reserve_backend_startup().await.unwrap();

    assert_ne!(reservation.port(), 0);
    assert!(
        backend_port_startup_lock().try_lock().is_err(),
        "a live reservation must exclude another in-process backend startup"
    );
    // SAFETY: the reservation owns this open descriptor while F_GETFD reads its
    // descriptor flags.
    let flags = unsafe { libc::fcntl(reservation._cross_process_guard.as_raw_fd(), libc::F_GETFD) };
    assert_ne!(flags, -1);
    assert_ne!(
        flags & libc::FD_CLOEXEC,
        0,
        "backend children must not inherit the startup lock"
    );

    let output = Command::new(std::env::current_exe().unwrap())
        .arg(TEST_NAME)
        .args(["--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, TEST_NAME)
        .output()
        .expect("exec current test executable for cross-process lock probe");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "cross-process lock probe failed: {}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
        output.status
    );
    assert!(
        stdout.contains("test result: ok. 1 passed; 0 failed;"),
        "cross-process lock probe did not run exactly one test:\n{stdout}"
    );

    drop(reservation);
    let lock_file = open_backend_startup_lock_file().unwrap();
    // SAFETY: lock_file owns a valid descriptor for this nonblocking lock
    // probe after the reservation has been dropped.
    assert_eq!(
        unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB,) },
        0,
        "dropping the reservation must release the cross-process lock"
    );
}
