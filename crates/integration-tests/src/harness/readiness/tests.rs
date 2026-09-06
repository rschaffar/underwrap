use super::*;
use std::process::Command;

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
