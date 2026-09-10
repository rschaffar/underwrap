use super::*;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::time::{Duration, Instant};

const HELPER_WAIT_TIMEOUT: Duration = Duration::from_secs(1);
const GATE_OBSERVE_TIMEOUT: Duration = Duration::from_secs(2);
const CHILD_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

fn observe_bytes(bytes: &[u8]) -> GateAuthorization {
    let gate = ExecGate::new_for_test().unwrap();
    let mut writer = gate.writer.file;
    let reader = gate.reader;
    writer.write_all(bytes).unwrap();
    drop(writer);
    reader.observe()
}

#[test]
fn exec_gate_accepts_only_exact_armed_then_eof() {
    assert_eq!(observe_bytes(ARMED), GateAuthorization::Accepted);
    assert_eq!(
        observe_bytes(b""),
        GateAuthorization::Suppressed(GateFailure::EofBeforeArmed)
    );
    assert_eq!(
        observe_bytes(FAILED),
        GateAuthorization::Suppressed(GateFailure::Failed)
    );
    assert_eq!(
        observe_bytes(&[ARMED, FAILED].concat()),
        GateAuthorization::Suppressed(GateFailure::Failed)
    );
}

#[test]
fn exec_gate_rejects_truncated_malformed_duplicate_and_trailing_streams() {
    for bytes in [
        &ARMED[..ARMED.len() - 1],
        b"Unknown\n".as_slice(),
        &[ARMED, ARMED].concat(),
        &[ARMED, b"x"].concat(),
        &FAILED[..FAILED.len() - 1],
    ] {
        assert_eq!(
            observe_bytes(bytes),
            GateAuthorization::Suppressed(GateFailure::Malformed),
            "stream {bytes:?} should be rejected"
        );
    }
}

#[test]
fn exec_gate_reports_reader_io_failure() {
    let directory = tempfile::TempDir::new().unwrap();
    let reader = GateReader {
        file: File::open(directory.path()).unwrap(),
    };
    assert_eq!(
        reader.observe(),
        GateAuthorization::Suppressed(GateFailure::Io)
    );
}

struct ChildGuard {
    pid: Option<libc::pid_t>,
}

impl ChildGuard {
    fn new(pid: libc::pid_t) -> Self {
        Self { pid: Some(pid) }
    }

    fn wait_bounded(&mut self, timeout: Duration) -> Result<libc::c_int, String> {
        let pid = self.pid.expect("child guard is armed");
        let deadline = Instant::now() + timeout;
        loop {
            let mut status = 0;
            let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if result == pid {
                self.pid = None;
                return Ok(status);
            }
            if result < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(format!("waitpid({pid}) failed: {error}"));
            }
            if Instant::now() >= deadline {
                return Err(format!("timed out waiting for child {pid}"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let Some(pid) = self.pid else {
            return;
        };
        unsafe { libc::kill(pid, libc::SIGKILL) };
        loop {
            let mut status = 0;
            let result = unsafe { libc::waitpid(pid, &mut status, 0) };
            if result == pid {
                self.pid = None;
                return;
            }
            if result < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                if error.raw_os_error() == Some(libc::ECHILD) {
                    self.pid = None;
                }
                return;
            }
        }
    }
}

fn observe_bounded(reader: GateReader, timeout: Duration) -> Result<GateAuthorization, String> {
    let deadline = Instant::now() + timeout;
    let mut poll_fd = libc::pollfd {
        fd: reader.file.as_raw_fd(),
        // POLLHUP and POLLERR are reported even when no events are requested.
        // Waiting for closure prevents the production blocking read from
        // stranding a test when a child unexpectedly retains the writer.
        events: 0,
        revents: 0,
    };
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for exec gate closure".to_string());
        }
        let timeout_ms = remaining.as_millis().clamp(1, i32::MAX as u128) as libc::c_int;
        poll_fd.revents = 0;
        let result = unsafe { libc::poll(&mut poll_fd, 1, timeout_ms) };
        if result == 0 {
            return Err("timed out waiting for exec gate closure".to_string());
        }
        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("polling exec gate failed: {error}"));
        }
        if poll_fd.revents & libc::POLLHUP != 0 {
            return Ok(reader.observe());
        }
        if poll_fd.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
            return Err(format!(
                "polling exec gate returned error events {:#x}",
                poll_fd.revents
            ));
        }
    }
}

#[test]
fn exec_gate_normal_returned_exec_error_reports_failed() {
    let gate = ExecGate::new_for_test().unwrap();
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        let mut writer = gate.into_child();
        if writer.send_armed().is_err() {
            unsafe { libc::_exit(2) };
        }
        let error = Command::new("/definitely/missing-underwrap-gate-target").exec();
        let _ = error;
        let _ = writer.send_failed();
        unsafe { libc::_exit(1) };
    }

    let mut child = ChildGuard::new(pid);
    let observation = observe_bounded(gate.into_parent(), GATE_OBSERVE_TIMEOUT).unwrap();
    let status = child.wait_bounded(CHILD_WAIT_TIMEOUT).unwrap();
    assert_eq!(
        observation,
        GateAuthorization::Suppressed(GateFailure::Failed)
    );
    assert!(libc::WIFEXITED(status));
    assert_eq!(libc::WEXITSTATUS(status), 1);
}

#[test]
fn exec_gate_cloexec_closure_accepts_executed_target() {
    let gate = ExecGate::new_for_test().unwrap();
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        let mut writer = gate.into_child();
        if writer.send_armed().is_err() {
            unsafe { libc::_exit(2) };
        }
        let error = Command::new("/bin/sh").arg("-c").arg("exit 0").exec();
        let _ = error;
        let _ = writer.send_failed();
        unsafe { libc::_exit(1) };
    }

    let mut child = ChildGuard::new(pid);
    let observation = observe_bounded(gate.into_parent(), GATE_OBSERVE_TIMEOUT).unwrap();
    let status = child.wait_bounded(CHILD_WAIT_TIMEOUT).unwrap();
    assert_eq!(observation, GateAuthorization::Accepted);
    assert!(libc::WIFEXITED(status));
    assert_eq!(libc::WEXITSTATUS(status), 0);
}

#[test]
fn exec_gate_parent_and_child_close_opposite_ends() {
    let gate = ExecGate::new_for_test().unwrap();
    let reader_fd = gate.reader.file.as_raw_fd();
    let writer_fd = gate.writer.file.as_raw_fd();
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        let writer = gate.into_child();
        let reader_closed = unsafe { libc::fcntl(reader_fd, libc::F_GETFD) } == -1;
        let writer_open = unsafe { libc::fcntl(writer.raw_fd(), libc::F_GETFD) } >= 0;
        unsafe { libc::_exit(if reader_closed && writer_open { 0 } else { 1 }) };
    }

    let mut child = ChildGuard::new(pid);
    let reader = gate.into_parent();
    assert_eq!(reader.file.as_raw_fd(), reader_fd);
    assert_eq!(unsafe { libc::fcntl(writer_fd, libc::F_GETFD) }, -1);
    let status = child.wait_bounded(CHILD_WAIT_TIMEOUT).unwrap();
    assert!(libc::WIFEXITED(status));
    assert_eq!(libc::WEXITSTATUS(status), 0);
}

fn helper_child_result(mut writer: GateWriter) -> i32 {
    let helper_pid = unsafe { libc::fork() };
    if helper_pid < 0 {
        return 2;
    }
    if helper_pid == 0 {
        drop(writer);
        unsafe { libc::_exit(0) };
    }

    let mut helper = ChildGuard::new(helper_pid);
    // Finish helper cleanup before the supervisor-side gate deadline, so even
    // a stuck helper cannot outlive a failed observation assertion.
    let status = match helper.wait_bounded(HELPER_WAIT_TIMEOUT) {
        Ok(status) => status,
        Err(_) => return 2,
    };
    if !libc::WIFEXITED(status) || libc::WEXITSTATUS(status) != 0 {
        return 2;
    }
    if writer.send_failed().is_err() {
        return 2;
    }
    1
}

#[test]
fn exec_gate_helper_does_not_retain_writer() {
    let gate = ExecGate::new_for_test().unwrap();
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        let writer = gate.into_child();
        let code = helper_child_result(writer);
        unsafe { libc::_exit(code) };
    }

    let mut child = ChildGuard::new(pid);
    let started = Instant::now();
    let observation = observe_bounded(gate.into_parent(), GATE_OBSERVE_TIMEOUT).unwrap();
    assert_eq!(
        observation,
        GateAuthorization::Suppressed(GateFailure::Failed)
    );
    assert!(started.elapsed() < GATE_OBSERVE_TIMEOUT);
    let status = child.wait_bounded(CHILD_WAIT_TIMEOUT).unwrap();
    assert!(libc::WIFEXITED(status));
    assert_eq!(libc::WEXITSTATUS(status), 1);
}

#[test]
fn exec_gate_creation_avoids_occupied_fixed_fds() {
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        let null = std::fs::File::open("/dev/null").unwrap().into_raw_fd();
        if unsafe { libc::dup2(null, EXEC_BUNDLE_FD) } < 0
            || unsafe { libc::dup2(null, WRAPPER_RESOURCE_CHANNEL_FD) } < 0
        {
            unsafe { libc::_exit(2) };
        }
        if null != EXEC_BUNDLE_FD && null != WRAPPER_RESOURCE_CHANNEL_FD {
            unsafe { libc::close(null) };
        }
        let result = ExecGate::new().map(|gate| {
            let reader = gate.reader.file.as_raw_fd();
            let writer = gate.writer.file.as_raw_fd();
            reader != EXEC_BUNDLE_FD
                && reader != WRAPPER_RESOURCE_CHANNEL_FD
                && writer != EXEC_BUNDLE_FD
                && writer != WRAPPER_RESOURCE_CHANNEL_FD
        });
        unsafe { libc::_exit(if result == Ok(true) { 0 } else { 1 }) };
    }

    let mut child = ChildGuard::new(pid);
    let status = child.wait_bounded(CHILD_WAIT_TIMEOUT).unwrap();
    assert!(libc::WIFEXITED(status));
    assert_eq!(libc::WEXITSTATUS(status), 0);
}

#[test]
fn exec_gate_writer_and_fd20_do_not_survive_target_exec() {
    let gate = ExecGate::new_for_test().unwrap();
    let writer_fd = gate.writer.raw_fd();
    let pipe_link = std::fs::read_link(format!("/proc/self/fd/{writer_fd}"))
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let script_dir = tempfile::TempDir::new().unwrap();
    let script = script_dir.path().join("probe.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n[ ! -e /proc/self/fd/20 ] || exit 20\nfor fd in /proc/self/fd/*; do [ \"$(readlink \"$fd\")\" != {pipe_link:?} ] || exit 21; done\nexit 0\n"
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();

    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        let mut writer = gate.into_child();
        let null = std::fs::File::open("/dev/null").unwrap().into_raw_fd();
        if unsafe { libc::dup2(null, WRAPPER_RESOURCE_CHANNEL_FD) } < 0 {
            unsafe { libc::_exit(2) };
        }
        if null != WRAPPER_RESOURCE_CHANNEL_FD {
            unsafe { libc::close(null) };
        }
        unsafe { libc::close(WRAPPER_RESOURCE_CHANNEL_FD) };
        if crate::boundary::close_inherited_fds_for_exec().is_err() || writer.send_armed().is_err()
        {
            unsafe { libc::_exit(2) };
        }
        let error = Command::new(&script).exec();
        let _ = error;
        let _ = writer.send_failed();
        unsafe { libc::_exit(1) };
    }

    let mut child = ChildGuard::new(pid);
    let observation = observe_bounded(gate.into_parent(), GATE_OBSERVE_TIMEOUT).unwrap();
    let status = child.wait_bounded(CHILD_WAIT_TIMEOUT).unwrap();
    assert_eq!(observation, GateAuthorization::Accepted);
    assert!(libc::WIFEXITED(status));
    assert_eq!(libc::WEXITSTATUS(status), 0);
}

#[test]
fn exec_gate_owned_fds_close_on_error_drop() {
    // Isolate numeric-fd reuse from parallel test threads: after fork this child
    // is single-threaded, so a successful F_GETFD cannot be an unrelated fd
    // allocated between RAII drop and the assertion.
    // SAFETY: the child performs only fd operations and exits with _exit.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        let gate = ExecGate::new_for_test().unwrap();
        let reader = gate.reader.file.into_raw_fd();
        let writer = gate.writer.file.into_raw_fd();
        // SAFETY: recreate each unique owner only to exercise its RAII drop.
        drop(unsafe { OwnedFd::from_raw_fd(reader) });
        drop(unsafe { OwnedFd::from_raw_fd(writer) });
        let closed = unsafe { libc::fcntl(reader, libc::F_GETFD) } == -1
            && unsafe { libc::fcntl(writer, libc::F_GETFD) } == -1;
        unsafe { libc::_exit(if closed { 0 } else { 1 }) }
    }
    let mut child = ChildGuard::new(pid);
    let status = child.wait_bounded(CHILD_WAIT_TIMEOUT).unwrap();
    assert!(libc::WIFEXITED(status));
    assert_eq!(libc::WEXITSTATUS(status), 0);
}
