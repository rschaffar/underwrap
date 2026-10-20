use super::*;
use std::net::Shutdown;
use std::os::unix::net::{UnixDatagram, UnixListener};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Stdio};
use underwrap_protocol::message::ResourceErrorCode;

const CHILD_MODE: &str = "UNDERWRAP_TEST_USAGE_CHILD";
const RESOURCE_ID: &str = "res_usage_regression";
const RESOURCE_NAME: &str = "test/usage-regression";

struct ProcessGuard(Child);

impl ProcessGuard {
    fn wait(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + CHILD_WAIT_TIMEOUT;
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "usage child did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// Re-exec avoids fork inheriting other tests' fd writers or libtest threads.
// Only the small simulated relay is forked in this fresh child process.
#[test]
fn usage_reporting_child() {
    let Ok(mode) = std::env::var(CHILD_MODE) else {
        return;
    };
    let root = PathBuf::from(std::env::var_os("UNDERWRAP_TEST_USAGE_ROOT").unwrap());
    crate::test_support::JOURNAL_SOCKET.with(|path| {
        *path.borrow_mut() = Some(root.join("journal"));
    });
    let channel_end = UnixStream::connect(root.join("resource")).unwrap();
    channel_end.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    channel_end
        .set_write_timeout(Some(TEST_IO_TIMEOUT))
        .unwrap();
    let (_staging, original, plans) = changed_staging_fixture(&[1; 16], &[2; 16]);
    let mut channel = test_resource_channel(
        channel_end.as_raw_fd(),
        original.to_canonical_bytes().unwrap(),
    );
    channel.resource_id = RESOURCE_ID.to_string();
    channel.resource_name = RESOURCE_NAME.to_string();
    channel.writable = mode == "checkin";

    let gate = ExecGate::new_for_test().unwrap();
    let liveness = supervision::LivenessPipe::new().unwrap();
    let target_status = target_status::TargetStatusPipe::new().unwrap();
    let status = if std::env::var_os("UNDERWRAP_TEST_USAGE_SIGNAL").is_some() {
        TargetStatus::Signaled(libc::SIGTERM)
    } else {
        TargetStatus::Exited(37)
    };
    // SAFETY: this is a fresh single-test process with no resource-service
    // thread. Each branch consumes its owned pipe endpoints before use.
    let relay = unsafe { libc::fork() };
    assert!(relay >= 0);
    if relay == 0 {
        drop(channel_end);
        let mut writer = gate.into_child();
        writer.send_armed().unwrap();
        drop(writer);
        let _liveness_reader = liveness.into_relay();
        target_status.into_relay().write(status).unwrap();
        // SAFETY: relay has completed its sole exact frame and must not run
        // libtest cleanup or unwind inherited host owners.
        unsafe { libc::_exit(0) }
    }
    let mut relay_guard = ChildGuard::new(relay);
    let isolation = target_uid::Isolation {
        // SAFETY: these queries take no arguments; same-user fsid windows
        // simulate collection, not deployment identity isolation.
        uid: unsafe { libc::getuid() },
        gid: unsafe { libc::getgid() },
    };
    // Transfer exclusive reap ownership to the production supervisor. A
    // returned diagnostic must not make the guard signal an already-reaped PID.
    relay_guard.reaped = true;
    let result = run_parent(
        relay,
        gate.into_parent(),
        liveness.into_host(),
        target_status.into_host(),
        &plans,
        &channel,
        true,
        isolation,
        None,
    );
    panic!("run_parent returned instead of propagating target status: {result:?}");
}

fn reporting_case(checkin: bool, reply: &str, suppressed: bool, signaled: bool, native: bool) {
    let root = tempfile::TempDir::new().unwrap();
    let listener = UnixListener::bind(root.path().join("resource")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let journal = native.then(|| UnixDatagram::bind(root.path().join("journal")).unwrap());
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "tests::writeback_usage::usage_reporting_child",
            "--nocapture",
        ])
        .env(CHILD_MODE, if checkin { "checkin" } else { "readonly" })
        .env("UNDERWRAP_TEST_USAGE_ROOT", root.path())
        .env(
            SUPPRESS_READONLY_WARNING_ENV,
            if suppressed { "1" } else { "0" },
        )
        .env_remove("UNDERWRAP_TEST_USAGE_SIGNAL")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if signaled {
        command.env("UNDERWRAP_TEST_USAGE_SIGNAL", "1");
    }
    let mut child = ProcessGuard(command.spawn().unwrap());
    let deadline = Instant::now() + CHILD_WAIT_TIMEOUT;
    let mut daemon = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    child.0.try_wait().unwrap().is_none(),
                    "child failed before connecting"
                );
                assert!(Instant::now() < deadline, "usage child did not connect");
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("accepting usage child: {error}"),
        }
    };
    daemon.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    daemon.set_write_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    if checkin {
        let request: WrapperResourceRequest =
            read_wrapper_resource_frame_sync(&mut daemon).unwrap();
        assert!(matches!(request, WrapperResourceRequest::Checkin { .. }));
        // No success/persistence response is supplied, only one denial.
        write_wrapper_resource_frame_sync(
            &mut daemon,
            &WrapperResourceResponse::Error {
                code: ResourceErrorCode::Unauthorized,
                message: "not writable".to_string(),
            },
        )
        .unwrap();
    }
    assert!(
        matches!(
            read_wrapper_resource_frame_sync::<WrapperResourceRequest>(&mut daemon).unwrap(),
            WrapperResourceRequest::UsageSignal,
        ),
        "expected UsageSignal, without displaying an unexpected resource body",
    );
    match reply {
        "transport" => daemon.shutdown(Shutdown::Write).unwrap(),
        "denied" | "internal" => write_wrapper_resource_frame_sync(
            &mut daemon,
            &WrapperResourceResponse::Error {
                code: if reply == "denied" {
                    ResourceErrorCode::Unauthorized
                } else {
                    ResourceErrorCode::Internal
                },
                // Deliberately arbitrary daemon text must never be displayed.
                message: "unsafe daemon text\nAUDIT=forged".to_string(),
            },
        )
        .unwrap(),
        "unexpected" => write_wrapper_resource_frame_sync(
            &mut daemon,
            &WrapperResourceResponse::CheckoutOk {
                blob: vec![0xa5; 32],
                writable: true,
            },
        )
        .unwrap(),
        "ok" => {
            write_wrapper_resource_frame_sync(&mut daemon, &WrapperResourceResponse::UsageSignalOk)
                .unwrap()
        }
        _ => unreachable!(),
    }
    let status = child.wait();
    if signaled {
        assert_eq!(status.signal(), Some(libc::SIGTERM));
    } else {
        assert_eq!(status.code(), Some(37));
    }
    // Exact EOF rules out an unintended Checkin or any follow-up/retry.
    assert_eq!(daemon.read(&mut [0]).unwrap(), 0);
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    let reason = if checkin {
        "server_rejected"
    } else {
        "permission_readonly"
    };
    let mut expected = String::new();
    if let Some(journal) = journal {
        journal.set_nonblocking(true).unwrap();
        let mut bytes = [0; 4096];
        let count = journal
            .recv(&mut bytes)
            .expect("missing structured rejection");
        let record = std::str::from_utf8(&bytes[..count]).unwrap();
        let expected_record = format!(
            "MESSAGE=writeback rejected for resource '{RESOURCE_NAME}' ({RESOURCE_ID}): {reason}\n\
             PRIORITY=4\nSYSLOG_IDENTIFIER=underwrap-ns\nAUDIT=writeback_rejected\n\
             REASON={reason}\nRESOURCE_ID={RESOURCE_ID}\nRESOURCE_NAME={RESOURCE_NAME}\n"
        );
        assert!(
            record == expected_record,
            "unexpected native rejection record"
        );
        assert_eq!(
            journal.recv(&mut bytes).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    } else {
        expected.push_str(&audit_json_line(reason, RESOURCE_ID, RESOURCE_NAME, None));
        expected.push('\n');
    }
    let error = match reply {
        "denied" => Some("daemon rejected UsageSignal (Unauthorized)"),
        "internal" => Some("daemon rejected UsageSignal (Internal)"),
        "transport" => Some("reading UsageSignal response failed"),
        "unexpected" => Some("unexpected UsageSignal response"),
        "ok" => None,
        _ => unreachable!(),
    };
    if let Some(error) = error {
        expected.push_str(&format!(
            "underwrap-ns: end-of-run usage reporting for resource '{RESOURCE_NAME}' ({RESOURCE_ID}): {error}\n"
        ));
    }
    if !suppressed {
        expected.push_str(&format!(
            "underwrap: changes to resource '{RESOURCE_NAME}' ({RESOURCE_ID}) were not persisted \
             (permission is read-only). Set {SUPPRESS_READONLY_WARNING_ENV}=1 to silence this message.\n"
        ));
    }
    // Exact output also excludes daemon text, response blobs, duplicate audit
    // records and any additional generic writeback failure replacing rejection.
    assert!(
        stderr == expected,
        "unexpected diagnostic output for {checkin}/{reply}/{suppressed}/{signaled}/{native}"
    );
}

fn reporting_matrix(checkin: bool) {
    for reply in ["denied", "transport", "ok", "internal", "unexpected"] {
        for suppressed in [false, true] {
            for signaled in [false, true] {
                for native in [false, true] {
                    reporting_case(checkin, reply, suppressed, signaled, native);
                }
            }
        }
    }
}

#[test]
fn known_readonly_rejection_survives_usage_failure_reporting_and_target_status() {
    reporting_matrix(false);
}

#[test]
fn known_checkin_rejection_survives_usage_failure_reporting_and_target_status() {
    reporting_matrix(true);
}
