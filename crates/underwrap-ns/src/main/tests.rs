use super::*;
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};
use underwrap_protocol::exec::ExecResourceBinding;
use underwrap_protocol::ipc::{
    read_wrapper_resource_frame_sync, write_wrapper_resource_frame_sync,
};
use underwrap_protocol::message::{SecretBytes, WrapperResourceRequest, WrapperResourceResponse};
use underwrap_protocol::resource_blob::{Collection, LibsecretSection, ResourceBlob};

const TEST_IO_TIMEOUT: Duration = Duration::from_secs(2);
const CHILD_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

fn bundle(access_paths: Vec<String>) -> ExecBundle {
    ExecBundle {
        env_passthrough: Vec::new(),
        env_set: HashMap::new(),
        target_runtime_dir_base: "/run/underwrap-runtime".to_string(),
        resource: ExecResourceBinding {
            resource_id: "res_x_y".to_string(),
            resource_name: "x/y".to_string(),
            access: underwrap_protocol::access::AccessMode::ReadOnly,
            env_projection: false,
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
        &ResourceBlob::empty(),
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
        libsecret_generation: None,
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
    let plans = [DirectoryMountPlan {
        mount_path: ".config/f006".to_string(),
        staging_path: staging.path().to_path_buf(),
    }];
    let (channel_end, mut daemon_end) = UnixStream::pair().unwrap();
    channel_end.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    daemon_end.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    write_wrapper_resource_frame_sync(&mut daemon_end, &response).unwrap();
    let mut channel = test_resource_channel(
        channel_end.as_raw_fd(),
        original.to_canonical_bytes().unwrap(),
    );
    channel.writable = writable;
    channel.libsecret = true;
    let input = WritebackInput {
        channel: &channel,
        directory_layout: &plans,
        libsecret_generation: Some(staged_libsecret),
    };

    let outcomes = dispatch_after_gate(GateAuthorization::Accepted, &[input]).unwrap();
    let request = read_wrapper_resource_frame_sync(&mut daemon_end).unwrap();
    (outcomes, request)
}

fn writeback_baseline_for_exact_candidate(candidate: &LibsecretSection) -> ResourceBlob {
    let mut complete = ResourceBlob::empty();
    complete.libsecret = candidate.clone();
    complete
        .files
        .insert("padding".into(), SecretBytes::from(Vec::<u8>::new()));
    let base = complete.to_canonical_bytes().unwrap().len();
    complete.files = BTreeMap::from([(
        format!(
            "padding{}",
            "x".repeat(underwrap_protocol::resource_blob::MAX_CANONICAL_RESOURCE_BYTES - base)
        ),
        SecretBytes::from(Vec::<u8>::new()),
    )]);
    assert_eq!(
        complete.to_canonical_bytes().unwrap().len(),
        underwrap_protocol::resource_blob::MAX_CANONICAL_RESOURCE_BYTES
    );
    complete.libsecret = LibsecretSection::default();
    complete
}

#[test]
fn complete_combined_writeback_exact_max_sends_bulk_checkin() {
    let mut candidate = LibsecretSection::default();
    candidate.collections.insert(
        "a".into(),
        Collection {
            label: "candidate".into(),
            created: 1_700_000_000,
            modified: 1_700_000_000,
            items: BTreeMap::new(),
        },
    );
    let original = writeback_baseline_for_exact_candidate(&candidate);
    let (channel_end, mut daemon_end) = UnixStream::pair().unwrap();
    daemon_end.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    daemon_end.set_write_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    let expected_candidate = candidate.clone();
    let responder = std::thread::spawn(move || {
        let request: WrapperResourceRequest =
            read_wrapper_resource_frame_sync(&mut daemon_end).unwrap();
        let WrapperResourceRequest::Checkin { blob } = request else {
            panic!("exact-max composition did not send Checkin");
        };
        assert_eq!(
            blob.len(),
            underwrap_protocol::resource_blob::MAX_CANONICAL_RESOURCE_BYTES
        );
        assert_eq!(
            ResourceBlob::from_canonical_bytes(&blob).unwrap().libsecret,
            expected_candidate
        );
        write_wrapper_resource_frame_sync(&mut daemon_end, &WrapperResourceResponse::CheckinOk)
            .unwrap();
        daemon_end.shutdown(std::net::Shutdown::Both).unwrap();
        drop(daemon_end);
    });
    let mut channel = test_resource_channel(
        channel_end.as_raw_fd(),
        original.to_canonical_bytes().unwrap(),
    );
    channel.files_target_paths.clear();
    channel.libsecret = true;
    let outcome = crate::writeback::dispatch_writeback(&[WritebackInput {
        channel: &channel,
        directory_layout: &[],
        libsecret_generation: Some(&candidate),
    }]);
    drop(channel);
    drop(channel_end);
    responder.join().unwrap();
    assert_eq!(outcome.unwrap(), vec![WritebackOutcome::Persisted]);
}

#[test]
fn complete_combined_writeback_one_over_max_sends_no_terminal_verb() {
    let mut exact = LibsecretSection::default();
    exact.collections.insert(
        "a".into(),
        Collection {
            label: "candidate".into(),
            created: 1_700_000_000,
            modified: 1_700_000_000,
            items: BTreeMap::new(),
        },
    );
    let original = writeback_baseline_for_exact_candidate(&exact);
    let mut oversized = exact;
    let collection = oversized.collections.remove("a").unwrap();
    oversized.collections.insert("aa".into(), collection);
    let (channel_end, mut daemon_end) = UnixStream::pair().unwrap();
    daemon_end.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    let mut channel = test_resource_channel(
        channel_end.as_raw_fd(),
        original.to_canonical_bytes().unwrap(),
    );
    channel.files_target_paths.clear();
    channel.libsecret = true;
    let outcomes = crate::writeback::dispatch_writeback(&[WritebackInput {
        channel: &channel,
        directory_layout: &[],
        libsecret_generation: Some(&oversized),
    }])
    .unwrap();
    assert_eq!(
        outcomes,
        vec![WritebackOutcome::RejectedTooLarge {
            resource_id: "res_f006_unit".to_string(),
            resource_name: "test/f006-unit".to_string(),
            attempted_len: underwrap_protocol::resource_blob::MAX_CANONICAL_RESOURCE_BYTES + 1,
            maximum_len: underwrap_protocol::resource_blob::MAX_CANONICAL_RESOURCE_BYTES,
        }]
    );
    let mut byte = [0];
    assert!(matches!(
        daemon_end.read(&mut byte).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ));
    drop(channel_end);
    drop(daemon_end);
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
fn old_same_uid_state_path_is_irrelevant_to_selected_libsecret() {
    let dir = tempfile::TempDir::new().unwrap();
    let old_path = dir.path().join("old-final-state");
    let mut attacker = LibsecretSection::default();
    attacker.aliases.insert(
        "attacker_selected".to_string(),
        "missing_collection".to_string(),
    );
    std::fs::write(&old_path, attacker.to_canonical_bytes().unwrap()).unwrap();

    let (channel_end, mut daemon_end) = UnixStream::pair().unwrap();
    daemon_end.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    write_wrapper_resource_frame_sync(&mut daemon_end, &WrapperResourceResponse::UsageSignalOk)
        .unwrap();
    let original = ResourceBlob::empty();
    let mut channel = test_resource_channel(
        channel_end.as_raw_fd(),
        original.to_canonical_bytes().unwrap(),
    );
    channel.libsecret = true;
    channel.files_target_paths.clear();
    let input = WritebackInput {
        channel: &channel,
        directory_layout: &[],
        libsecret_generation: Some(&original.libsecret),
    };

    assert_eq!(
        dispatch_after_gate(GateAuthorization::Accepted, &[input]).unwrap(),
        Some(vec![WritebackOutcome::Unchanged])
    );
    assert_eq!(
        read_wrapper_resource_frame_sync::<WrapperResourceRequest>(&mut daemon_end).unwrap(),
        WrapperResourceRequest::UsageSignal
    );
    assert_eq!(
        LibsecretSection::from_canonical_bytes(&std::fs::read(old_path).unwrap()).unwrap(),
        attacker
    );
}

#[test]
fn run_parent_without_publication_preserves_nonempty_libsecret_and_propagates_status() {
    let mut original = ResourceBlob::empty();
    original.libsecret.collections.insert(
        "main".to_string(),
        Collection {
            label: "Main collection".to_string(),
            created: 1,
            modified: 2,
            items: BTreeMap::new(),
        },
    );
    original
        .libsecret
        .aliases
        .insert("default".to_string(), "main".to_string());
    let original_canonical = original.to_canonical_bytes().unwrap();
    original.libsecret.validate_publication_semantics().unwrap();

    let (channel_end, mut daemon_end) = UnixStream::pair().unwrap();
    daemon_end.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    write_wrapper_resource_frame_sync(&mut daemon_end, &WrapperResourceResponse::UsageSignalOk)
        .unwrap();
    let mut channel = test_resource_channel(channel_end.as_raw_fd(), original_canonical);
    channel.libsecret = true;
    channel.files_target_paths.clear();

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
            let mut gate_writer = gate.into_child();
            gate_writer.send_armed().unwrap();
            drop(gate_writer);
            let _liveness_reader = liveness.into_relay();
            target_status
                .into_relay()
                .write(TargetStatus::Exited(37))
                .unwrap();
            unsafe { libc::_exit(0) }
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
            &[],
            &channel,
            false,
            isolation,
            None,
        );
        eprintln!("publication-disabled lifecycle returned: {result:?}");
        unsafe { libc::_exit(1) }
    }

    drop(channel_end);
    let mut guard = ChildGuard::new(outer);
    assert_eq!(
        read_wrapper_resource_frame_sync::<WrapperResourceRequest>(&mut daemon_end).unwrap(),
        WrapperResourceRequest::UsageSignal,
        "an unprojected libsecret slot must remain byte-identical"
    );
    let status = guard.wait_bounded(CHILD_WAIT_TIMEOUT).unwrap();
    assert!(libc::WIFEXITED(status), "outer status {status}");
    assert_eq!(libc::WEXITSTATUS(status), 37, "outer status {status}");
    let mut byte = [0];
    assert_eq!(daemon_end.read(&mut byte).unwrap(), 0);
}

#[test]
fn lifecycle_conjunction_retained_publisher_suppresses_terminal_but_propagates_status() {
    let (channel_end, mut daemon_end) = UnixStream::pair().unwrap();
    daemon_end.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    let mut channel = test_resource_channel(
        channel_end.as_raw_fd(),
        ResourceBlob::empty().to_canonical_bytes().unwrap(),
    );
    channel.libsecret = true;
    channel.files_target_paths.clear();

    let outer = unsafe { libc::fork() };
    assert!(outer >= 0);
    if outer == 0 {
        drop(daemon_end);
        let publication = PublicationPair::new().unwrap();
        let retained_publisher = publication.publisher.try_clone().unwrap();
        let gate = ExecGate::new_for_test().unwrap();
        let liveness = supervision::LivenessPipe::new().unwrap();
        let target_status = target_status::TargetStatusPipe::new().unwrap();
        let relay = unsafe { libc::fork() };
        if relay == 0 {
            drop(publication.host);
            drop(retained_publisher);
            let mut publisher = publication.publisher;
            let section = LibsecretSection::default().to_canonical_bytes().unwrap();
            publisher
                .write_all(
                    &underwrap_protocol::libsecret_publication::encode_request_header(
                        0,
                        section.len(),
                    )
                    .unwrap(),
                )
                .unwrap();
            publisher.write_all(&section).unwrap();
            let mut response = [0; underwrap_protocol::libsecret_publication::RESPONSE_LEN];
            publisher.read_exact(&mut response).unwrap();
            assert_eq!(
                underwrap_protocol::libsecret_publication::decode_response(&response).unwrap(),
                (
                    underwrap_protocol::libsecret_publication::ResponseKind::Ack,
                    0
                )
            );
            let mut gate_writer = gate.into_child();
            gate_writer.send_armed().unwrap();
            drop(gate_writer);
            let _liveness_reader = liveness.into_relay();
            target_status
                .into_relay()
                .write(TargetStatus::Exited(37))
                .unwrap();
            unsafe { libc::_exit(0) }
        }
        drop(publication.publisher);
        // Deliberately retain the duplicate outside the relay namespace until
        // exact status propagation exits this process.
        let _retained_publisher = retained_publisher;
        let isolation = target_uid::Isolation {
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
        };
        let result = run_parent(
            relay,
            gate.into_parent(),
            liveness.into_host(),
            target_status.into_host(),
            &[],
            &channel,
            false,
            isolation,
            Some(publication.host),
        );
        eprintln!("retained publication lifecycle returned: {result:?}");
        unsafe { libc::_exit(1) }
    }

    drop(channel_end);
    let mut guard = ChildGuard::new(outer);
    let status = guard.wait_bounded(CHILD_WAIT_TIMEOUT).unwrap();
    assert!(libc::WIFEXITED(status), "outer status {status}");
    assert_eq!(libc::WEXITSTATUS(status), 37, "outer status {status}");
    let mut byte = [0];
    assert_eq!(daemon_end.read(&mut byte).unwrap(), 0);
}

fn initial_publication_failure_case(semantic_reject: bool) {
    let marker_dir = tempfile::TempDir::new().unwrap();
    let marker = marker_dir.path().join("target-ran");
    let (channel_end, mut daemon_end) = UnixStream::pair().unwrap();
    daemon_end.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
    let mut channel = test_resource_channel(
        channel_end.as_raw_fd(),
        ResourceBlob::empty().to_canonical_bytes().unwrap(),
    );
    channel.libsecret = true;
    channel.files_target_paths.clear();

    let outer = unsafe { libc::fork() };
    assert!(outer >= 0);
    if outer == 0 {
        drop(daemon_end);
        let publication = PublicationPair::new().unwrap();
        let gate = ExecGate::new_for_test().unwrap();
        let liveness = supervision::LivenessPipe::new().unwrap();
        let target_status = target_status::TargetStatusPipe::new().unwrap();
        let relay = unsafe { libc::fork() };
        if relay == 0 {
            drop(publication.host);
            let mut publisher = publication.publisher;
            if semantic_reject {
                let mut invalid = LibsecretSection::default();
                invalid.aliases.insert("bad".into(), "missing".into());
                let body = invalid.to_canonical_bytes().unwrap();
                publisher
                    .write_all(
                        &underwrap_protocol::libsecret_publication::encode_request_header(
                            0,
                            body.len(),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                publisher.write_all(&body).unwrap();
                let mut response = [0; underwrap_protocol::libsecret_publication::RESPONSE_LEN];
                publisher.read_exact(&mut response).unwrap();
                let decoded =
                    underwrap_protocol::libsecret_publication::decode_response(&response).unwrap();
                if decoded.0 == underwrap_protocol::libsecret_publication::ResponseKind::Ack {
                    std::fs::write(&marker, b"target executed").unwrap();
                }
                assert_eq!(
                    decoded,
                    (
                        underwrap_protocol::libsecret_publication::ResponseKind::Reject,
                        0
                    )
                );
            } else {
                publisher.write_all(b"partial").unwrap();
            }
            drop(publisher);
            gate.into_child().send_failed().unwrap();
            let _liveness_reader = liveness.into_relay();
            target_status
                .into_relay()
                .write(TargetStatus::Exited(if semantic_reject { 23 } else { 24 }))
                .unwrap();
            unsafe { libc::_exit(0) }
        }
        drop(publication.publisher);
        let isolation = target_uid::Isolation {
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
        };
        let result = run_parent(
            relay,
            gate.into_parent(),
            liveness.into_host(),
            target_status.into_host(),
            &[],
            &channel,
            false,
            isolation,
            Some(publication.host),
        );
        eprintln!("initial publication failure returned: {result:?}");
        unsafe { libc::_exit(1) }
    }

    drop(channel_end);
    let mut guard = ChildGuard::new(outer);
    let status = guard.wait_bounded(CHILD_WAIT_TIMEOUT).unwrap();
    assert!(libc::WIFEXITED(status), "outer status {status}");
    assert_eq!(
        libc::WEXITSTATUS(status),
        if semantic_reject { 23 } else { 24 }
    );
    assert!(
        !marker.exists(),
        "target marker exists after initial failure"
    );
    let mut byte = [0];
    assert_eq!(daemon_end.read(&mut byte).unwrap(), 0);
}

#[test]
fn initial_publication_reject_and_channel_failure_are_pre_exec_no_terminal() {
    initial_publication_failure_case(true);
    initial_publication_failure_case(false);
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
    write_wrapper_resource_frame_sync(&mut daemon_end, &WrapperResourceResponse::CheckinOk)
        .unwrap();
    let (_staging, blob, plans) = changed_staging_fixture(b"before", b"after");
    let mut channel =
        test_resource_channel(channel_end.as_raw_fd(), blob.to_canonical_bytes().unwrap());
    channel.writable = true;
    channel.libsecret = true;

    // Isolate run_parent because exact target-status propagation exits.
    let outer = unsafe { libc::fork() };
    assert!(outer >= 0);
    if outer == 0 {
        drop(daemon_end);
        let gate = ExecGate::new_for_test().unwrap();
        let liveness = supervision::LivenessPipe::new().unwrap();
        let target_status = target_status::TargetStatusPipe::new().unwrap();
        let publication = PublicationPair::new().unwrap();
        let relay = unsafe { libc::fork() };
        if relay == 0 {
            drop(publication.host);
            let mut publisher = publication.publisher;
            let section = LibsecretSection::default().to_canonical_bytes().unwrap();
            publisher
                .write_all(
                    &underwrap_protocol::libsecret_publication::encode_request_header(
                        0,
                        section.len(),
                    )
                    .unwrap(),
                )
                .unwrap();
            publisher.write_all(&section).unwrap();
            let mut response = [0; underwrap_protocol::libsecret_publication::RESPONSE_LEN];
            publisher.read_exact(&mut response).unwrap();
            assert_eq!(
                underwrap_protocol::libsecret_publication::decode_response(&response).unwrap(),
                (
                    underwrap_protocol::libsecret_publication::ResponseKind::Ack,
                    0
                )
            );
            drop(publisher);
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
        drop(publication.publisher);
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
            Some(publication.host),
        );
        eprintln!("lifecycle conjunction returned: {result:?}");
        unsafe { libc::_exit(1) }
    }

    drop(channel_end);
    let request_result = if matches!(case, LifecycleCase::Accepted) {
        Some(read_wrapper_resource_frame_sync(&mut daemon_end))
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
fn audit_json_string_and_display_escape_arbitrary_utf8_metadata() {
    let metadata = "quote\" slash/ backslash\\ newline\n carriage\r tab\t backspace\u{08} \
                    formfeed\u{0c} nul\0 unit\u{1f} Unicode ☃";
    assert_eq!(
        json_string(metadata),
        "\"quote\\\" slash/ backslash\\\\ newline\\n carriage\\r tab\\t backspace\\b \
         formfeed\\f nul\\u0000 unit\\u001f Unicode ☃\""
    );
    assert_eq!(
        one_line_display(metadata),
        "quote\\\" slash/ backslash\\\\ newline\\n carriage\\r tab\\t backspace\\b \
         formfeed\\f nul\\u0000 unit\\u001f Unicode ☃"
    );

    let all_c0: String = (0_u8..=0x1f).map(char::from).collect();
    let encoded_c0 = json_string(&all_c0);
    assert!(
        encoded_c0[1..encoded_c0.len() - 1]
            .chars()
            .all(|character| character > '\u{1f}')
    );
    assert!(encoded_c0.contains(r#"\u0001"#));
    assert!(encoded_c0.contains(r#"\u001f"#));
}

#[test]
fn audit_json_lines_cover_rejection_reasons_and_paired_size_fields() {
    let resource_id = "id\"\\\n\u{1f}☃";
    let resource_name = "name\"\\\n\u{1f}☃";
    let maximum = underwrap_protocol::resource_blob::MAX_CANONICAL_RESOURCE_BYTES;

    for (reason, size) in [
        ("permission_readonly", None),
        ("server_rejected", None),
        ("resource_too_large", Some((maximum + 1, maximum))),
    ] {
        let line = audit_json_line(reason, resource_id, resource_name, size);
        assert!(line.starts_with(&format!(
            r#"{{"audit":"writeback_rejected","reason":"{reason}""#
        )));
        assert!(line.contains(r#""resource_id":"id\"\\\n\u001f☃""#));
        assert!(line.contains(r#""resource_name":"name\"\\\n\u001f☃""#));
        assert!(
            !line.contains('\n'),
            "fallback must remain one physical line"
        );
        match size {
            Some((attempted, maximum)) => {
                assert!(line.contains(&format!("\"maximum_length\":{maximum}")));
                assert!(line.contains(&format!("\"attempted_length\":{attempted}")));
            }
            None => {
                assert!(!line.contains("maximum_length"));
                assert!(!line.contains("attempted_length"));
            }
        }
    }
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
