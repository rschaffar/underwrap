use super::*;
use std::collections::HashMap;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};
use underwrap_protocol::exec::ExecResourceBinding;
use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{SecretBytes, WrapperResourceRequest, WrapperResourceResponse};
use underwrap_protocol::resource_blob::{LibsecretSection, ResourceBlob};

const TEST_IO_TIMEOUT: Duration = Duration::from_secs(2);
const CHILD_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

fn bundle(access_paths: Vec<String>) -> ExecBundle {
    ExecBundle {
        env: HashMap::new(),
        env_passthrough: Vec::new(),
        env_set: HashMap::new(),
        target_runtime_dir_base: "/run/underwrap-runtime".to_string(),
        resource: ExecResourceBinding {
            resource_id: "res_x_y".to_string(),
            resource_name: "x/y".to_string(),
            access: underwrap_protocol::config::AccessMode::ReadOnly,
            libsecret: false,
            files_target_paths: Vec::new(),
        },
        access_paths,
    }
}

#[test]
fn target_runtime_dir_is_base_plus_target_user_name() {
    // ns derives the executing uid's own dir as <base>/<user-name>. uid 0 maps
    // to "root" on every Unix, so this is a portable check of the scheme.
    assert_eq!(
        target_runtime_dir("/run/underwrap-runtime", 0).unwrap(),
        "/run/underwrap-runtime/root",
    );
}

#[test]
fn target_runtime_dir_rejects_relative_base() {
    assert!(target_runtime_dir("run/underwrap-runtime", 0).is_err());
}

#[test]
fn build_target_env_uses_the_resolved_runtime_dir_verbatim() {
    // build_target_env does not derive the runtime dir; it exports exactly the
    // value run() resolved (the target's own provisioned dir).
    let b = bundle(Vec::new());
    let env = crate::env::build_target_env(
        &HashMap::new(),
        &b,
        Path::new("/home/alice"),
        "/run/underwrap-runtime/underwrap-target-abc",
    )
    .unwrap();
    assert_eq!(
        env.get("XDG_RUNTIME_DIR").map(String::as_str),
        Some("/run/underwrap-runtime/underwrap-target-abc"),
    );
}

fn observed_gate(marker: &[u8]) -> GateAuthorization {
    let (reader, mut writer) = ExecGate::new_for_test().unwrap().into_ends();
    if marker == b"Armed\n" {
        writer.send_armed().unwrap();
    } else if marker == b"Failed\n" {
        writer.send_failed().unwrap();
    } else {
        panic!("unsupported marker fixture: {marker:?}");
    }
    drop(writer);
    reader.observe()
}

fn test_resource_channel(fd: i32, original_canonical: Vec<u8>) -> ResourceChannel {
    ResourceChannel {
        resource_id: "res_f006_unit".to_string(),
        resource_name: "test/f006-unit".to_string(),
        channel_fd: fd,
        original_canonical,
        writable: true,
        libsecret: false,
        files_target_paths: vec![".config/f006".to_string()],
    }
}

fn changed_staging_fixture(
    original: &[u8],
    changed: &[u8],
) -> (tempfile::TempDir, ResourceBlob, [DirectoryMountPlan; 1]) {
    let staging = tempfile::TempDir::new().unwrap();
    std::fs::write(staging.path().join("token"), changed).unwrap();
    let mut blob = ResourceBlob::empty();
    blob.files.insert(
        ".config/f006/token".to_string(),
        SecretBytes::from(original.to_vec()),
    );
    let plans = [DirectoryMountPlan {
        mount_path: ".config/f006".to_string(),
        staging_path: staging.path().to_path_buf(),
    }];
    (staging, blob, plans)
}

#[test]
fn pre_exec_failure_skips_collection_and_terminal_verbs() {
    let (channel_end, mut daemon_end) = UnixStream::pair().unwrap();
    daemon_end.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    let (_staging, blob, plans) = changed_staging_fixture(b"original", b"changed");
    assert_eq!(
        std::fs::read(plans[0].staging_path.join("token")).unwrap(),
        b"changed"
    );
    assert_eq!(
        blob.files[".config/f006/token"].as_slice(),
        b"original",
        "Checkout and staging must witness a real change"
    );
    let channel =
        test_resource_channel(channel_end.as_raw_fd(), blob.to_canonical_bytes().unwrap());
    let input = WritebackInput {
        channel: &channel,
        directory_layout: &plans,
        libsecret_state_path: None,
    };

    let authorization = observed_gate(b"Failed\n");
    assert_eq!(dispatch_after_gate(authorization, &[input]).unwrap(), None);
    drop(channel_end);
    let mut byte = [0_u8; 1];
    assert_eq!(daemon_end.read(&mut byte).unwrap(), 0);
}

struct ChildGuard {
    pid: libc::pid_t,
    reaped: bool,
}

impl ChildGuard {
    fn new(pid: libc::pid_t) -> Self {
        Self { pid, reaped: false }
    }

    fn wait_bounded(&mut self, timeout: Duration) -> Result<libc::c_int, String> {
        let deadline = Instant::now() + timeout;
        loop {
            let mut status = 0;
            let result = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
            if result == self.pid {
                self.reaped = true;
                return Ok(status);
            }
            if result < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(format!("waitpid({}) failed: {error}", self.pid));
            }
            if Instant::now() >= deadline {
                return Err(format!("timed out waiting for child {}", self.pid));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        unsafe { libc::kill(self.pid, libc::SIGKILL) };
        loop {
            let mut status = 0;
            let result = unsafe { libc::waitpid(self.pid, &mut status, 0) };
            if result == self.pid
                || (result < 0
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD))
            {
                self.reaped = true;
                return;
            }
            if result < 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                return;
            }
        }
    }
}

fn run_target_liveness_wait_harness(reader: supervision::LivenessReader, hang: bool) -> i32 {
    let target_pid = unsafe { libc::fork() };
    if target_pid < 0 {
        return 10;
    }
    if target_pid == 0 {
        drop(reader);
        if hang {
            loop {
                unsafe { libc::pause() };
            }
        }
        unsafe { libc::_exit(37) }
    }

    let mut target = ChildGuard::new(target_pid);
    let started = Instant::now();
    let result = wait_for_target_with_liveness(target_pid, &reader);
    // Do not leak a deliberately hanging target if the production wait returns
    // an unexpected result; ChildGuard supplies bounded kill-and-reap cleanup.
    match result {
        Ok(status) if !hang && libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 37 => {
            target.reaped = true;
            // SAFETY: isolate the production helper's PDEATHSIG side effect in
            // this forked harness and clear it before reporting success.
            if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, 0) } == 0 {
                0
            } else {
                11
            }
        }
        Err(error)
            if hang
                && error.contains("parent liveness ended")
                && started.elapsed() < Duration::from_secs(2) =>
        {
            0
        }
        _ => 12,
    }
}

fn assert_target_liveness_wait(hang: bool) {
    let liveness = supervision::LivenessPipe::new().unwrap();
    let relay = unsafe { libc::fork() };
    assert!(relay >= 0);
    if relay == 0 {
        let reader = liveness.into_relay();
        let code = run_target_liveness_wait_harness(reader, hang);
        unsafe { libc::_exit(code) }
    }

    let mut relay = ChildGuard::new(relay);
    if hang {
        drop(liveness);
    } else {
        let _writer = liveness.into_host();
        let status = relay.wait_bounded(CHILD_WAIT_TIMEOUT).unwrap();
        assert!(libc::WIFEXITED(status), "relay status {status}");
        assert_eq!(libc::WEXITSTATUS(status), 0, "relay status {status}");
        return;
    }
    let status = relay.wait_bounded(CHILD_WAIT_TIMEOUT).unwrap();
    assert!(libc::WIFEXITED(status), "relay status {status}");
    assert_eq!(libc::WEXITSTATUS(status), 0, "relay status {status}");
}

#[test]
fn target_liveness_wait_returns_exact_healthy_status() {
    assert_target_liveness_wait(false);
}

#[test]
fn target_liveness_wait_rejects_hup_and_cleans_up_target() {
    assert_target_liveness_wait(true);
}

fn run_parent_wait_failure_case(channel: &ResourceChannel, plans: &[DirectoryMountPlan]) -> i32 {
    let gate = match ExecGate::new_for_test() {
        Ok(gate) => gate,
        Err(_) => return 10,
    };
    let target_pid = unsafe { libc::fork() };
    if target_pid < 0 {
        return 11;
    }
    if target_pid == 0 {
        let mut writer = gate.into_child();
        let code = if writer.send_armed().is_ok() { 0 } else { 12 };
        unsafe { libc::_exit(code) };
    }

    let reader = gate.into_parent();
    let mut target = ChildGuard::new(target_pid);
    let target_status = match target.wait_bounded(CHILD_WAIT_TIMEOUT) {
        Ok(status) => status,
        Err(_) => return 13,
    };
    if !libc::WIFEXITED(target_status) || libc::WEXITSTATUS(target_status) != 0 {
        return 14;
    }

    let isolation = target_uid::Isolation {
        uid: unsafe { libc::getuid() },
        gid: unsafe { libc::getgid() },
    };
    let liveness = match supervision::LivenessPipe::new() {
        Ok(pipe) => pipe.into_host(),
        Err(_) => return 15,
    };
    let status_reader = match target_status::TargetStatusPipe::new() {
        Ok(pipe) => pipe.into_host(),
        Err(_) => return 16,
    };
    match run_parent(
        target_pid,
        reader,
        liveness,
        status_reader,
        plans,
        channel,
        true,
        isolation,
        None,
    ) {
        Err(error) if error.contains("waitpid failed") => 0,
        _ => 15,
    }
}

#[test]
fn run_parent_wait_failure_after_armed_skips_collection_and_terminal_verbs() {
    let (channel_end, mut daemon_end) = UnixStream::pair().unwrap();
    daemon_end.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    let (_staging, blob, plans) = changed_staging_fixture(b"original", b"changed");
    assert_eq!(
        std::fs::read(plans[0].staging_path.join("token")).unwrap(),
        b"changed"
    );
    assert_eq!(
        blob.files[".config/f006/token"].as_slice(),
        b"original",
        "Checkout and staging must witness a real change"
    );
    let channel =
        test_resource_channel(channel_end.as_raw_fd(), blob.to_canonical_bytes().unwrap());

    let outer_pid = unsafe { libc::fork() };
    assert!(outer_pid >= 0, "outer fork failed");
    if outer_pid == 0 {
        drop(daemon_end);
        let code = run_parent_wait_failure_case(&channel, &plans);
        unsafe { libc::_exit(code) };
    }

    let mut outer = ChildGuard::new(outer_pid);
    let status = outer.wait_bounded(CHILD_WAIT_TIMEOUT).unwrap();
    assert!(
        libc::WIFEXITED(status),
        "outer process did not exit: {status}"
    );
    assert_eq!(libc::WEXITSTATUS(status), 0, "outer process failed");
    drop(channel_end);
    let mut byte = [0_u8; 1];
    assert_eq!(daemon_end.read(&mut byte).unwrap(), 0);
}

fn accepted_combined_dispatch(
    original: &ResourceBlob,
    staged_file: &[u8],
    staged_libsecret: &LibsecretSection,
    writable: bool,
    response: WrapperResourceResponse,
) -> (Option<Vec<WritebackOutcome>>, WrapperResourceRequest) {
    let staging = tempfile::TempDir::new().unwrap();
    std::fs::write(staging.path().join("token"), staged_file).unwrap();
    let state = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(state.path(), staged_libsecret.to_canonical_bytes().unwrap()).unwrap();
    let plans = [DirectoryMountPlan {
        mount_path: ".config/f006".to_string(),
        staging_path: staging.path().to_path_buf(),
    }];
    let (channel_end, mut daemon_end) = UnixStream::pair().unwrap();
    channel_end.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    daemon_end.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    write_frame_sync(&mut daemon_end, &response).unwrap();
    let mut channel = test_resource_channel(
        channel_end.as_raw_fd(),
        original.to_canonical_bytes().unwrap(),
    );
    channel.writable = writable;
    channel.libsecret = true;
    let input = WritebackInput {
        channel: &channel,
        directory_layout: &plans,
        libsecret_state_path: Some(state.path()),
    };

    let outcomes = dispatch_after_gate(GateAuthorization::Accepted, &[input]).unwrap();
    let request = read_frame_sync(&mut daemon_end).unwrap();
    (outcomes, request)
}

#[test]
fn accepted_dispatch_preserves_combined_files_libsecret_and_env_semantics() {
    let mut original = ResourceBlob::empty();
    original.env.insert(
        "F006_ENV".to_string(),
        SecretBytes::from(b"preserved-env".to_vec()),
    );
    original.files.insert(
        ".config/f006/token".to_string(),
        SecretBytes::from(b"original-file".to_vec()),
    );
    original
        .libsecret
        .aliases
        .insert("default".to_string(), "original-alias".to_string());

    let (outcomes, request) = accepted_combined_dispatch(
        &original,
        b"original-file",
        &original.libsecret,
        true,
        WrapperResourceResponse::UsageSignalOk,
    );
    assert_eq!(outcomes, Some(vec![WritebackOutcome::Unchanged]));
    assert_eq!(request, WrapperResourceRequest::UsageSignal);

    let mut changed_libsecret = original.libsecret.clone();
    changed_libsecret
        .aliases
        .insert("default".to_string(), "changed-alias".to_string());
    let (outcomes, request) = accepted_combined_dispatch(
        &original,
        b"changed-file",
        &changed_libsecret,
        true,
        WrapperResourceResponse::CheckinOk,
    );
    assert_eq!(outcomes, Some(vec![WritebackOutcome::Persisted]));
    let WrapperResourceRequest::Checkin { blob } = request else {
        panic!("changed writable projection did not send Checkin");
    };
    let checked_in = ResourceBlob::from_canonical_bytes(&blob).unwrap();
    assert_eq!(checked_in.env, original.env);
    assert_eq!(
        checked_in.files[".config/f006/token"].as_slice(),
        b"changed-file"
    );
    assert_eq!(checked_in.libsecret, changed_libsecret);

    let (outcomes, request) = accepted_combined_dispatch(
        &original,
        b"changed-file",
        &changed_libsecret,
        false,
        WrapperResourceResponse::UsageSignalOk,
    );
    assert_eq!(request, WrapperResourceRequest::UsageSignal);
    assert_eq!(
        outcomes,
        Some(vec![WritebackOutcome::DroppedReadOnly {
            resource_id: "res_f006_unit".to_string(),
            resource_name: "test/f006-unit".to_string(),
        }])
    );
}

#[test]
fn exec_gate_wait_failure_is_reported() {
    let error = wait_for_child(libc::pid_t::MAX).unwrap_err();
    assert!(
        error.contains("waitpid failed"),
        "unexpected error: {error}"
    );
}

#[derive(Clone, Copy)]
enum LifecycleCase {
    Accepted,
    GateFailed,
    InvalidStatus,
    RelayNonzero,
    RelaySignaled,
}

fn lifecycle_conjunction_case(
    case: LifecycleCase,
) -> (libc::c_int, Option<WrapperResourceRequest>) {
    let (channel_end, mut daemon_end) = UnixStream::pair().unwrap();
    daemon_end.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    write_frame_sync(&mut daemon_end, &WrapperResourceResponse::CheckinOk).unwrap();
    let (_staging, blob, plans) = changed_staging_fixture(b"before", b"after");
    let mut channel =
        test_resource_channel(channel_end.as_raw_fd(), blob.to_canonical_bytes().unwrap());
    channel.writable = true;

    // Isolate run_parent because exact target-status propagation exits.
    let outer = unsafe { libc::fork() };
    assert!(outer >= 0);
    if outer == 0 {
        drop(daemon_end);
        let gate = ExecGate::new_for_test().unwrap();
        let liveness = supervision::LivenessPipe::new().unwrap();
        let target_status = target_status::TargetStatusPipe::new().unwrap();
        let relay = unsafe { libc::fork() };
        if relay == 0 {
            let mut writer = gate.into_child();
            let _liveness_reader = liveness.into_relay();
            let status_writer = target_status.into_relay();
            if matches!(case, LifecycleCase::GateFailed) {
                writer.send_failed().unwrap();
            } else {
                writer.send_armed().unwrap();
            }
            drop(writer);
            match case {
                LifecycleCase::InvalidStatus => {
                    status_writer.write_raw_for_test(b"bad").unwrap();
                }
                LifecycleCase::RelaySignaled => {
                    status_writer
                        .write(TargetStatus::Signaled(libc::SIGTERM))
                        .unwrap();
                    unsafe { libc::raise(libc::SIGKILL) };
                }
                _ => status_writer.write(TargetStatus::Exited(37)).unwrap(),
            }
            unsafe {
                libc::_exit(if matches!(case, LifecycleCase::RelayNonzero) {
                    2
                } else {
                    0
                })
            }
        }
        let isolation = target_uid::Isolation {
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
        };
        let result = run_parent(
            relay,
            gate.into_parent(),
            liveness.into_host(),
            target_status.into_host(),
            &plans,
            &channel,
            false,
            isolation,
            None,
        );
        eprintln!("lifecycle conjunction returned: {result:?}");
        unsafe { libc::_exit(1) }
    }

    drop(channel_end);
    let request_result = if matches!(case, LifecycleCase::Accepted) {
        Some(read_frame_sync(&mut daemon_end))
    } else {
        None
    };
    let mut guard = ChildGuard::new(outer);
    let status = guard.wait_bounded(CHILD_WAIT_TIMEOUT).unwrap();
    let request = if let Some(result) = request_result {
        Some(result.unwrap_or_else(|error| panic!("status {status}: {error}")))
    } else {
        let mut byte = [0_u8; 1];
        match daemon_end.read(&mut byte) {
            Ok(0) => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
            other => panic!("invalid lifecycle cell wrote request data: {other:?}"),
        }
        None
    };
    (status, request)
}

#[test]
fn lifecycle_conjunction_requires_gate_status_and_clean_relay_for_dispatch() {
    let (status, request) = lifecycle_conjunction_case(LifecycleCase::Accepted);
    assert!(libc::WIFEXITED(status));
    assert_eq!(libc::WEXITSTATUS(status), 37);
    assert!(matches!(
        request,
        Some(WrapperResourceRequest::Checkin { .. })
    ));

    let (status, request) = lifecycle_conjunction_case(LifecycleCase::GateFailed);
    assert!(libc::WIFEXITED(status));
    assert_eq!(libc::WEXITSTATUS(status), 37);
    assert!(request.is_none(), "invalid gate dispatched a terminal verb");

    for case in [
        LifecycleCase::InvalidStatus,
        LifecycleCase::RelayNonzero,
        LifecycleCase::RelaySignaled,
    ] {
        let (status, request) = lifecycle_conjunction_case(case);
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 1);
        assert!(
            request.is_none(),
            "invalid relay/status dispatched a terminal verb"
        );
    }
}
