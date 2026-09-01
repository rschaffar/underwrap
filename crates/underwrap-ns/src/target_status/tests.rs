use super::*;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::process::Command;

// EOF is a process-wide descriptor-table observation. A concurrently spawned
// test child can inherit a writer before exec, so run EOF witnesses in a fresh
// single-test process whose own pipe cannot cross that boundary.
const ISOLATED_TEST_ENV: &str = "UNDERWRAP_TARGET_STATUS_ISOLATED_TEST";

fn exec_isolated(test_name: &str, body: impl FnOnce()) {
    if std::env::var(ISOLATED_TEST_ENV).ok().as_deref() == Some(test_name) {
        body();
        return;
    }

    let output = Command::new(std::env::current_exe().unwrap())
        .arg(test_name)
        .args(["--exact", "--nocapture", "--test-threads=1"])
        .env(ISOLATED_TEST_ENV, test_name)
        .output()
        .expect("exec current test executable for target-status probe");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "target-status probe failed: {}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
        output.status
    );
    assert!(
        stdout.contains("test result: ok. 1 passed; 0 failed;"),
        "target-status probe did not run exactly one test:\n{stdout}"
    );
}

#[test]
fn valid_status_frames_round_trip() {
    exec_isolated(
        "target_status::tests::valid_status_frames_round_trip",
        || {
            for expected in [
                TargetStatus::Exited(0),
                TargetStatus::Exited(255),
                TargetStatus::Signaled(libc::SIGTERM),
            ] {
                let pipe = TargetStatusPipe::new().unwrap();
                let TargetStatusPipe { reader, writer } = pipe;
                writer.write(expected).unwrap();
                assert_eq!(reader.read_after_clean_relay().unwrap(), expected);
            }
        },
    );
}

fn parse_bytes(bytes: &[u8]) -> Result<TargetStatus, String> {
    let TargetStatusPipe { reader, writer } = TargetStatusPipe::new().unwrap();
    // SAFETY: bytes is valid for this bounded test write and writer owns fd.
    assert_eq!(
        unsafe { libc::write(writer.0.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) },
        bytes.len() as isize
    );
    drop(writer);
    reader.read_after_clean_relay()
}

#[test]
fn malformed_truncated_and_extra_frames_fail_closed() {
    exec_isolated(
        "target_status::tests::malformed_truncated_and_extra_frames_fail_closed",
        || {
            for bytes in [
                Vec::new(),
                b"U".to_vec(),
                b"BAD!\x01\x01\x00".to_vec(),
                b"UWTS\x02\x01\x00".to_vec(),
                b"UWTS\x01\x03\x00".to_vec(),
                b"UWTS\x01\x02\x00".to_vec(),
                b"UWTS\x01\x01\x00x".to_vec(),
            ] {
                assert!(parse_bytes(&bytes).is_err(), "accepted {bytes:?}");
            }
        },
    );
}

#[test]
fn signal_frame_above_runtime_sigrtmax_fails_closed() {
    exec_isolated(
        "target_status::tests::signal_frame_above_runtime_sigrtmax_fails_closed",
        || {
            let signal = libc::SIGRTMAX() + 1;
            assert!(signal <= u8::MAX as libc::c_int);
            let frame = [b'U', b'W', b'T', b'S', 1, SIGNALED, signal as u8];
            let error = parse_bytes(&frame).unwrap_err();
            assert!(error.contains("invalid signal"), "{error}");

            let TargetStatusPipe { reader: _, writer } = TargetStatusPipe::new().unwrap();
            let error = writer.write(TargetStatus::Signaled(signal)).unwrap_err();
            assert!(error.contains("invalid target signal"), "{error}");
        },
    );
}

#[test]
fn target_status_reports_reader_io_failure() {
    let directory = tempfile::TempDir::new().unwrap();
    let reader = TargetStatusReader(File::open(directory.path()).unwrap().into());
    let error = reader.read_after_clean_relay().unwrap_err();
    assert!(error.contains("reading target-status frame"), "{error}");
}

#[test]
fn retained_writer_is_eagain_not_a_hang() {
    let TargetStatusPipe { reader, writer } = TargetStatusPipe::new().unwrap();
    let error = reader.read_after_clean_relay().unwrap_err();
    assert!(error.contains("remains open"), "{error}");
    drop(writer);
}
