use super::*;
use std::fs::File;
use std::os::fd::AsRawFd;

#[test]
fn valid_status_frames_round_trip() {
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
}

#[test]
fn signal_frame_above_runtime_sigrtmax_fails_closed() {
    let signal = libc::SIGRTMAX() + 1;
    assert!(signal <= u8::MAX as libc::c_int);
    let frame = [b'U', b'W', b'T', b'S', 1, SIGNALED, signal as u8];
    let error = parse_bytes(&frame).unwrap_err();
    assert!(error.contains("invalid signal"), "{error}");

    let TargetStatusPipe { reader: _, writer } = TargetStatusPipe::new().unwrap();
    let error = writer.write(TargetStatus::Signaled(signal)).unwrap_err();
    assert!(error.contains("invalid target signal"), "{error}");
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
