use super::*;
use std::cell::RefCell;
use std::fs::File;
use std::io::Read;
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn require_absolute_env_path_accepts_absolute_paths() {
    let _guard = env_lock().lock().unwrap();
    unsafe {
        std::env::set_var("UNDERWRAP_TEST_BIN", "/run/current-system/sw/bin/test-bin");
    }

    let path = require_absolute_env_path("UNDERWRAP_TEST_BIN").unwrap();
    assert_eq!(path, "/run/current-system/sw/bin/test-bin");

    unsafe {
        std::env::remove_var("UNDERWRAP_TEST_BIN");
    }
}

#[test]
fn require_absolute_env_path_rejects_missing_and_relative_paths() {
    let _guard = env_lock().lock().unwrap();
    unsafe {
        std::env::remove_var("UNDERWRAP_TEST_BIN");
    }
    let err = require_absolute_env_path("UNDERWRAP_TEST_BIN").unwrap_err();
    assert!(err.contains("not set"));

    unsafe {
        std::env::set_var("UNDERWRAP_TEST_BIN", "relative-bin");
    }
    let err = require_absolute_env_path("UNDERWRAP_TEST_BIN").unwrap_err();
    assert!(err.contains("must be an absolute path"));

    unsafe {
        std::env::remove_var("UNDERWRAP_TEST_BIN");
    }
}

fn find_in_path(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join(bin))
            .find(|p| p.is_file())
    })
}

/// `apply_helper_env` must clear the inherited (caller-derived) environment
/// and reintroduce only the trusted whitelist, so a caller-controlled
/// service-discovery variable like `XDG_DATA_DIRS` cannot reach a helper.
#[test]
fn apply_helper_env_clears_inherited_and_keeps_whitelist() {
    let _guard = env_lock().lock().unwrap();

    let Some(env_bin) = find_in_path("env") else {
        eprintln!("skipping: no `env` binary on PATH");
        return;
    };

    // Pollute the process environment with a caller-controlled variable; it
    // must not survive into the helper command.
    unsafe {
        std::env::set_var("XDG_DATA_DIRS", "/tmp/attacker-controlled");
    }

    let mut helper_env = HashMap::new();
    helper_env.insert("HOME".to_string(), "/home/trusted".to_string());
    helper_env.insert("PATH".to_string(), "/run/current-system/sw/bin".to_string());

    let mut cmd = Command::new(&env_bin);
    apply_helper_env(&mut cmd, &helper_env);
    let output = cmd.output().expect("run env binary");

    unsafe {
        std::env::remove_var("XDG_DATA_DIRS");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("XDG_DATA_DIRS"),
        "caller XDG_DATA_DIRS leaked into helper env:\n{stdout}"
    );
    assert!(
        stdout.contains("HOME=/home/trusted"),
        "expected whitelisted HOME in helper env:\n{stdout}"
    );
    assert!(
        stdout.contains("PATH=/run/current-system/sw/bin"),
        "expected whitelisted PATH in helper env:\n{stdout}"
    );
}

fn assert_open_and_cloexec(fd: RawFd) {
    let flags = fd_flags(fd).unwrap();
    assert_ne!(flags & libc::FD_CLOEXEC, 0, "fd {fd} lost CLOEXEC");
    assert_usable(fd);
}

fn assert_open_without_cloexec(fd: RawFd) {
    let flags = fd_flags(fd).unwrap();
    assert_eq!(flags & libc::FD_CLOEXEC, 0, "fd {fd} retained CLOEXEC");
    assert_usable(fd);
}

fn assert_usable(fd: RawFd) {
    // SAFETY: fd is expected to be live; F_DUPFD_CLOEXEC returns a new owned
    // descriptor on success and does not consume the original.
    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    assert!(
        duplicate >= 0,
        "fd {fd} is not usable: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: fcntl returned this fresh descriptor, transferring sole ownership.
    drop(unsafe { OwnedFd::from_raw_fd(duplicate) });
}

fn assert_closed(fd: RawFd) {
    // SAFETY: F_GETFD only inspects the child-local descriptor table.
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EBADF)
    );
}

fn dbus_test_endpoints() -> (OwnedFd, DbusChildEndpoints) {
    let (gate_read, gate_write) = cloexec_pipe().unwrap();
    drop(gate_read);
    let (ready_read, ready_write) = cloexec_pipe().unwrap();
    let (address_read, address_write) = cloexec_pipe().unwrap();
    let (init_read, init_write) = cloexec_pipe().unwrap();
    let (publisher, host) = UnixStream::pair().unwrap();
    drop(host);
    let exec_gate_fd = gate_write.as_raw_fd();
    (
        gate_write,
        DbusChildEndpoints {
            exec_gate_fd,
            ready_read,
            ready_write,
            address_read,
            address_write,
            init_read,
            init_write,
            publisher,
        },
    )
}

fn secretsd_test_endpoints() -> (OwnedFd, SecretsdChildEndpoints) {
    let (gate_read, gate_write) = cloexec_pipe().unwrap();
    drop(gate_read);
    let (ready_read, ready_write) = cloexec_pipe().unwrap();
    let (init_read, init_write) = cloexec_pipe().unwrap();
    let (publisher, host) = UnixStream::pair().unwrap();
    drop(host);
    let exec_gate_fd = gate_write.as_raw_fd();
    (
        gate_write,
        SecretsdChildEndpoints {
            exec_gate_fd,
            ready_read,
            ready_write,
            init_read,
            init_write,
            publisher,
        },
    )
}

fn assert_dbus_child_endpoint_selection() {
    let (gate, endpoints) = dbus_test_endpoints();
    let unwanted = [
        endpoints.ready_read.as_raw_fd(),
        endpoints.ready_write.as_raw_fd(),
        endpoints.address_read.as_raw_fd(),
        endpoints.init_read.as_raw_fd(),
        endpoints.init_write.as_raw_fd(),
        endpoints.publisher.as_raw_fd(),
    ];
    let gate_fd = gate.as_raw_fd();

    prepare_dbus_child(endpoints, |selected| {
        for fd in unwanted {
            assert_closed(fd);
        }
        assert_open_and_cloexec(gate_fd);
        assert_open_without_cloexec(selected.address_write.as_raw_fd());
        Ok(())
    })
    .unwrap();
}

// A parallel test can reuse a just-closed process-global fd number before
// `assert_closed` observes it, so make the numeric closure witness exec-isolated.
#[test]
fn dbus_child_consumes_only_unwanted_endpoints_and_selects_address_writer() {
    assert_exec_probe(ExecProbeMode::DbusSelection, 0);
}

fn assert_secretsd_child_endpoint_selection() {
    let (gate, endpoints) = secretsd_test_endpoints();
    let unwanted = [
        endpoints.ready_read.as_raw_fd(),
        endpoints.init_write.as_raw_fd(),
    ];
    let gate_fd = gate.as_raw_fd();

    prepare_secretsd_child(endpoints, |selected| {
        for fd in unwanted {
            assert_closed(fd);
        }
        assert_open_and_cloexec(gate_fd);
        assert_open_without_cloexec(selected.init_read.as_raw_fd());
        assert_open_without_cloexec(selected.ready_write.as_raw_fd());
        assert_open_without_cloexec(selected.publisher.as_raw_fd());
        Ok(())
    })
    .unwrap();
}

// Keep the sibling numeric closure witness under the same isolated fd table.
#[test]
fn secretsd_child_consumes_only_unwanted_endpoints_and_selects_three_writers() {
    assert_exec_probe(ExecProbeMode::SecretsdSelection, 0);
}

const EXEC_PROBE_TEST: &str = "libsecret::tests::exec_isolated_probe";
const EXEC_PROBE_MODE_ENV: &str = "UNDERWRAP_LIBSECRET_TEST_PROBE_MODE";
const EXEC_PROBE_MARKER_FD_ENV: &str = "UNDERWRAP_LIBSECRET_TEST_PROBE_MARKER_FD";
const EXEC_PROBE_SENTINEL_EXIT: i32 = 200;

#[derive(Clone, Copy)]
enum ExecProbeMode {
    DbusError,
    DbusPanic,
    DbusSelection,
    SecretsdError,
    SecretsdPanic,
    SecretsdSelection,
    ReusedFdUnwind,
}

impl ExecProbeMode {
    fn name(self) -> &'static str {
        match self {
            Self::DbusError => "dbus-error",
            Self::DbusPanic => "dbus-panic",
            Self::DbusSelection => "dbus-selection",
            Self::SecretsdError => "secretsd-error",
            Self::SecretsdPanic => "secretsd-panic",
            Self::SecretsdSelection => "secretsd-selection",
            Self::ReusedFdUnwind => "reused-fd-unwind",
        }
    }

    fn marker(self) -> &'static [u8] {
        match self {
            Self::DbusError => b"underwrap-libsecret-probe:dbus-error\n",
            Self::DbusPanic => b"underwrap-libsecret-probe:dbus-panic\n",
            Self::DbusSelection => b"underwrap-libsecret-probe:dbus-selection\n",
            Self::SecretsdError => b"underwrap-libsecret-probe:secretsd-error\n",
            Self::SecretsdPanic => b"underwrap-libsecret-probe:secretsd-panic\n",
            Self::SecretsdSelection => b"underwrap-libsecret-probe:secretsd-selection\n",
            Self::ReusedFdUnwind => b"underwrap-libsecret-probe:reused-fd-unwind\n",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "dbus-error" => Some(Self::DbusError),
            "dbus-panic" => Some(Self::DbusPanic),
            "dbus-selection" => Some(Self::DbusSelection),
            "secretsd-error" => Some(Self::SecretsdError),
            "secretsd-panic" => Some(Self::SecretsdPanic),
            "secretsd-selection" => Some(Self::SecretsdSelection),
            "reused-fd-unwind" => Some(Self::ReusedFdUnwind),
            _ => None,
        }
    }
}

fn direct_write_all(fd: RawFd, mut bytes: &[u8]) -> bool {
    while !bytes.is_empty() {
        // SAFETY: bytes points to readable memory for its full length, and fd is
        // the inherited marker pipe selected before the probe creates endpoints.
        let written =
            unsafe { libc::write(fd, bytes.as_ptr().cast::<libc::c_void>(), bytes.len()) };
        if written > 0 {
            bytes = &bytes[written as usize..];
            continue;
        }
        if written < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
        {
            continue;
        }
        return false;
    }
    true
}

fn probe_mode_and_mark_entry() -> Option<ExecProbeMode> {
    let value = std::env::var(EXEC_PROBE_MODE_ENV).ok()?;
    let mode = ExecProbeMode::parse(&value).unwrap_or_else(|| {
        // SAFETY: an invalid internal probe selector must not return to the test harness.
        unsafe { libc::_exit(EXEC_PROBE_SENTINEL_EXIT) }
    });
    let marker_fd = std::env::var(EXEC_PROBE_MARKER_FD_ENV)
        .ok()
        .and_then(|value| value.parse::<RawFd>().ok())
        .unwrap_or_else(|| {
            // SAFETY: a selected probe without its marker capability is invalid.
            unsafe { libc::_exit(EXEC_PROBE_SENTINEL_EXIT) }
        });
    if !direct_write_all(marker_fd, mode.marker()) {
        // SAFETY: a probe that cannot prove entry must not continue or unwind.
        unsafe { libc::_exit(EXEC_PROBE_SENTINEL_EXIT) }
    }
    // SAFETY: marker_fd is an inherited raw descriptor owned only by this probe.
    unsafe { libc::close(marker_fd) };
    Some(mode)
}

fn assert_exec_probe(mode: ExecProbeMode, expected_status: i32) {
    let (marker_read, marker_write) = cloexec_pipe().unwrap();
    set_cloexec(marker_write.as_raw_fd(), false).unwrap();

    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg(EXEC_PROBE_TEST)
        .arg("--exact")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env(EXEC_PROBE_MODE_ENV, mode.name())
        .env(
            EXEC_PROBE_MARKER_FD_ENV,
            marker_write.as_raw_fd().to_string(),
        )
        .spawn()
        .expect("exec current test executable for libsecret probe");
    drop(marker_write);

    let status = child.wait().expect("wait for libsecret exec probe");
    let mut marker = Vec::new();
    File::from(marker_read)
        .read_to_end(&mut marker)
        .expect("read libsecret exec probe marker");
    assert_eq!(marker, mode.marker(), "exec probe helper did not enter");
    assert_eq!(
        status.code(),
        Some(expected_status),
        "unexpected status from {} probe: {status}",
        mode.name()
    );
}

fn reused_fd_survives_unwind() -> bool {
    let (_gate, endpoints) = dbus_test_endpoints();
    let reused_number = endpoints.ready_read.as_raw_fd();
    let source = File::open("/dev/null").unwrap();
    let replacement = Rc::new(RefCell::new(None::<OwnedFd>));
    let replacement_for_action = Rc::clone(&replacement);

    let result = catch_unwind(AssertUnwindSafe(|| {
        let _ = prepare_dbus_child(endpoints, |_| -> Result<(), String> {
            // SAFETY: source remains live, and the unwanted endpoint owner was
            // consumed before this callback. F_DUPFD_CLOEXEC creates ownership.
            let duplicated =
                unsafe { libc::fcntl(source.as_raw_fd(), libc::F_DUPFD_CLOEXEC, reused_number) };
            assert_eq!(duplicated, reused_number, "fd number was not reused");
            // SAFETY: fcntl returned this fresh descriptor with sole ownership.
            *replacement_for_action.borrow_mut() =
                Some(unsafe { OwnedFd::from_raw_fd(duplicated) });
            panic!("injected unwind after fd reuse");
        });
    }));
    if result.is_ok() {
        return false;
    }

    let replacement = replacement.borrow();
    let Some(replacement) = replacement.as_ref() else {
        return false;
    };
    if fd_flags(replacement.as_raw_fd()).unwrap_or(0) & libc::FD_CLOEXEC == 0 {
        return false;
    }
    if std::fs::read_link(format!("/proc/self/fd/{}", replacement.as_raw_fd())).ok()
        != Some(PathBuf::from("/dev/null"))
    {
        return false;
    }
    let mut duplicate = File::from(replacement.try_clone().unwrap());
    let mut byte = [0_u8; 1];
    duplicate.read(&mut byte).ok() == Some(0)
}

#[test]
fn exec_isolated_probe() {
    let Some(mode) = probe_mode_and_mark_entry() else {
        return;
    };

    if matches!(
        mode,
        ExecProbeMode::DbusSelection | ExecProbeMode::SecretsdSelection
    ) {
        let passed = catch_unwind(AssertUnwindSafe(|| match mode {
            ExecProbeMode::DbusSelection => assert_dbus_child_endpoint_selection(),
            ExecProbeMode::SecretsdSelection => assert_secretsd_child_endpoint_selection(),
            _ => unreachable!(),
        }))
        .is_ok();
        // SAFETY: this exec-isolated probe reports an exact bounded success/failure status.
        unsafe { libc::_exit(if passed { 0 } else { 1 }) }
    }

    if matches!(mode, ExecProbeMode::ReusedFdUnwind) {
        let passed = catch_unwind(AssertUnwindSafe(reused_fd_survives_unwind)).unwrap_or(false);
        // SAFETY: this exec-isolated probe reports an exact bounded success/failure status.
        unsafe { libc::_exit(if passed { 0 } else { 1 }) }
    }

    let _escaped = catch_unwind(AssertUnwindSafe(|| match mode {
        ExecProbeMode::DbusError => {
            let (_gate, endpoints) = dbus_test_endpoints();
            run_dbus_child(endpoints, |_| Err("injected dbus child error".to_string()));
        }
        ExecProbeMode::DbusPanic => {
            let (_gate, endpoints) = dbus_test_endpoints();
            run_dbus_child(endpoints, |_| -> Result<(), String> {
                panic!("injected dbus child panic");
            });
        }
        ExecProbeMode::SecretsdError => {
            let (_gate, endpoints) = secretsd_test_endpoints();
            run_secretsd_child(endpoints, |_| {
                Err("injected secretsd child error".to_string())
            });
        }
        ExecProbeMode::SecretsdPanic => {
            let (_gate, endpoints) = secretsd_test_endpoints();
            run_secretsd_child(endpoints, |_| -> Result<(), String> {
                panic!("injected secretsd child panic");
            });
        }
        ExecProbeMode::DbusSelection
        | ExecProbeMode::SecretsdSelection
        | ExecProbeMode::ReusedFdUnwind => unreachable!(),
    }));
    // SAFETY: reaching this point means endpoint construction panicked outside
    // the production boundary or a no-return production runner unexpectedly returned.
    unsafe { libc::_exit(EXEC_PROBE_SENTINEL_EXIT) }
}

#[test]
fn dbus_child_controlled_error_exits_without_returning() {
    assert_exec_probe(ExecProbeMode::DbusError, CHILD_ERROR_EXIT);
}

#[test]
fn dbus_child_panic_exits_without_returning() {
    assert_exec_probe(ExecProbeMode::DbusPanic, CHILD_PANIC_EXIT);
}

#[test]
fn secretsd_child_controlled_error_exits_without_returning() {
    assert_exec_probe(ExecProbeMode::SecretsdError, CHILD_ERROR_EXIT);
}

#[test]
fn secretsd_child_panic_exits_without_returning() {
    assert_exec_probe(ExecProbeMode::SecretsdPanic, CHILD_PANIC_EXIT);
}

#[test]
fn consumed_endpoint_owner_cannot_close_reused_fd_during_unwind() {
    assert_exec_probe(ExecProbeMode::ReusedFdUnwind, 0);
}
