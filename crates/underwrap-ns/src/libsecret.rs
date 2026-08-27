//! Boot a private dbus-daemon and `underwrap-secretsd` inside the
//! namespace and point the target at the private bus.
//!
//! The libsecret keyring for this run is provided as JCS-canonical
//! section bytes (loaded by ns from the resource blob's `libsecret`
//! section at Checkout time). ns writes those bytes into a one-shot
//! pipe and passes the read end as `--init-fd` to secretsd; secretsd
//! holds the section entirely in memory and persists each D-Bus
//! mutation on the private publication capability and waits for the host's
//! exact generation-matched response.

use std::collections::HashMap;
use std::io::Write;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::Command;

use crate::supervision::cloexec_pipe;

pub(crate) fn require_absolute_env_path(var_name: &str) -> Result<String, String> {
    let value = std::env::var(var_name)
        .map_err(|_| format!("{var_name} not set (required for libsecret mode)"))?;
    if !value.starts_with('/') {
        return Err(format!(
            "{var_name} must be an absolute path, got: {value:?}"
        ));
    }
    Ok(value)
}

/// Apply a sanitised environment to a helper `Command`: clear the inherited
/// (caller-derived) environment, then reintroduce only the trusted whitelist.
/// This keeps caller-controlled service-discovery variables (`XDG_DATA_DIRS`,
/// `XDG_RUNTIME_DIR`, `DBUS_*`, `LD_*`) out of the private bus daemon.
fn apply_helper_env(cmd: &mut Command, helper_env: &HashMap<String, String>) {
    cmd.env_clear();
    for (key, value) in helper_env {
        cmd.env(key, value);
    }
}

fn fd_flags(fd: RawFd) -> Result<i32, String> {
    // SAFETY: F_GETFD reads flags from the live descriptor named by fd.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(flags)
}

fn require_cloexec(fd: RawFd) -> Result<(), String> {
    if fd_flags(fd)? & libc::FD_CLOEXEC == 0 {
        return Err(format!("descriptor {fd} is not close-on-exec"));
    }
    Ok(())
}

fn set_cloexec(fd: RawFd, cloexec: bool) -> Result<(), String> {
    let flags = fd_flags(fd)?;
    let updated = if cloexec {
        flags | libc::FD_CLOEXEC
    } else {
        flags & !libc::FD_CLOEXEC
    };
    // SAFETY: F_SETFD changes only descriptor flags; `updated` preserves every
    // bit except the deliberately selected CLOEXEC state.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, updated) } < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

const CHILD_ERROR_EXIT: i32 = 1;
const CHILD_PANIC_EXIT: i32 = 101;

struct DbusChildEndpoints {
    exec_gate_fd: RawFd,
    ready_read: OwnedFd,
    ready_write: OwnedFd,
    address_read: OwnedFd,
    address_write: OwnedFd,
    init_read: OwnedFd,
    init_write: OwnedFd,
    publisher: UnixStream,
}

struct DbusSelectedEndpoint {
    address_write: OwnedFd,
}

struct SecretsdChildEndpoints {
    exec_gate_fd: RawFd,
    ready_read: OwnedFd,
    ready_write: OwnedFd,
    init_read: OwnedFd,
    init_write: OwnedFd,
    publisher: UnixStream,
}

struct SecretsdSelectedEndpoints {
    ready_write: OwnedFd,
    init_read: OwnedFd,
    publisher: UnixStream,
}

fn prepare_dbus_child(
    endpoints: DbusChildEndpoints,
    exec: impl FnOnce(DbusSelectedEndpoint) -> Result<(), String>,
) -> Result<(), String> {
    let DbusChildEndpoints {
        exec_gate_fd,
        ready_read,
        ready_write,
        address_read,
        address_write,
        init_read,
        init_write,
        publisher,
    } = endpoints;

    drop(ready_read);
    drop(ready_write);
    drop(address_read);
    drop(init_read);
    drop(init_write);
    drop(publisher);

    require_cloexec(exec_gate_fd)
        .map_err(|error| format!("verifying dbus-daemon exec gate: {error}"))?;
    set_cloexec(address_write.as_raw_fd(), false)
        .map_err(|error| format!("preparing dbus address fd: {error}"))?;
    exec(DbusSelectedEndpoint { address_write })
}

fn prepare_secretsd_child(
    endpoints: SecretsdChildEndpoints,
    exec: impl FnOnce(SecretsdSelectedEndpoints) -> Result<(), String>,
) -> Result<(), String> {
    let SecretsdChildEndpoints {
        exec_gate_fd,
        ready_read,
        ready_write,
        init_read,
        init_write,
        publisher,
    } = endpoints;

    drop(ready_read);
    drop(init_write);

    require_cloexec(exec_gate_fd)
        .map_err(|error| format!("verifying secretsd exec gate: {error}"))?;
    for fd in [
        init_read.as_raw_fd(),
        ready_write.as_raw_fd(),
        publisher.as_raw_fd(),
    ] {
        set_cloexec(fd, false)
            .map_err(|error| format!("preparing secretsd inherited fd {fd}: {error}"))?;
    }
    exec(SecretsdSelectedEndpoints {
        ready_write,
        init_read,
        publisher,
    })
}

fn run_dbus_child(
    endpoints: DbusChildEndpoints,
    exec: impl FnOnce(DbusSelectedEndpoint) -> Result<(), String>,
) -> ! {
    // The child is single-threaded and never resumes or exposes captured state.
    // Every action from the first endpoint consumption through diagnostics and
    // returned-exec handling is contained by this boundary.
    match catch_unwind(AssertUnwindSafe(|| {
        match prepare_dbus_child(endpoints, exec) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("underwrap-ns: {error}");
                CHILD_ERROR_EXIT
            }
        }
    })) {
        // SAFETY: both outcomes terminate the post-fork helper child without
        // running inherited destructors. The panic payload remains undropped.
        Ok(status) => unsafe { libc::_exit(status) },
        Err(_panic_payload) => unsafe { libc::_exit(CHILD_PANIC_EXIT) },
    }
}

fn run_secretsd_child(
    endpoints: SecretsdChildEndpoints,
    exec: impl FnOnce(SecretsdSelectedEndpoints) -> Result<(), String>,
) -> ! {
    // The child is single-threaded and never resumes or exposes captured state.
    // Every action from the first endpoint consumption through diagnostics and
    // returned-exec handling is contained by this boundary.
    match catch_unwind(AssertUnwindSafe(|| {
        match prepare_secretsd_child(endpoints, exec) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("underwrap-ns: {error}");
                CHILD_ERROR_EXIT
            }
        }
    })) {
        // SAFETY: both outcomes terminate the post-fork helper child without
        // running inherited destructors. The panic payload remains undropped.
        Ok(status) => unsafe { libc::_exit(status) },
        Err(_panic_payload) => unsafe { libc::_exit(CHILD_PANIC_EXIT) },
    }
}

pub(crate) fn setup_libsecret(
    cmd: &mut Command,
    exec_gate_fd: i32,
    helper_env: &HashMap<String, String>,
    section_bytes: &[u8],
    publisher: UnixStream,
) -> Result<(), String> {
    let dbus_daemon = require_absolute_env_path("UNDERWRAP_DBUS_DAEMON")?;
    let secretsd = require_absolute_env_path("UNDERWRAP_SECRETSD")?;

    let bus_dir = "/tmp/.underwrap-ns-bus";
    let bus_socket = format!("{bus_dir}/bus");
    let bus_address = format!("unix:path={bus_socket}");

    let (ready_read, ready_write) = cloexec_pipe()
        .map_err(|error| format!("pipe2(O_CLOEXEC) for readiness failed: {error}"))?;
    let (dbus_addr_read, dbus_addr_write) = cloexec_pipe()
        .map_err(|error| format!("pipe2(O_CLOEXEC) for dbus address failed: {error}"))?;
    // One-shot init pipe carrying the libsecret section bytes.
    let (init_read, init_write) = cloexec_pipe()
        .map_err(|error| format!("pipe2(O_CLOEXEC) for libsecret init failed: {error}"))?;

    let dbus_pid = unsafe { libc::fork() };
    if dbus_pid < 0 {
        return Err(format!(
            "fork dbus-daemon failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if dbus_pid == 0 {
        run_dbus_child(
            DbusChildEndpoints {
                exec_gate_fd,
                ready_read,
                ready_write,
                address_read: dbus_addr_read,
                address_write: dbus_addr_write,
                init_read,
                init_write,
                publisher,
            },
            |selected| {
                use std::os::unix::process::CommandExt;
                let address_fd = selected.address_write.as_raw_fd();
                let mut dbus_cmd = Command::new(&dbus_daemon);
                dbus_cmd
                    .arg("--session")
                    .arg(format!("--address={bus_address}"))
                    .arg("--nofork")
                    .arg("--nopidfile")
                    .arg(format!("--print-address={address_fd}"))
                    .stderr({
                        let log = std::fs::File::create(format!("{bus_dir}/dbus-daemon.log"))
                            .unwrap_or_else(|_| std::fs::File::open("/dev/null").unwrap());
                        std::process::Stdio::from(log)
                    });
                apply_helper_env(&mut dbus_cmd, helper_env);
                let error = dbus_cmd.exec();
                Err(format!("exec dbus-daemon failed: {error}"))
            },
        );
    }

    drop(dbus_addr_write);

    let actual_bus_address = {
        use std::io::BufRead;
        let file = std::fs::File::from(dbus_addr_read);
        let mut reader = std::io::BufReader::new(file);
        let mut addr = String::new();
        reader
            .read_line(&mut addr)
            .map_err(|e| format!("reading dbus-daemon address: {e}"))?;
        addr.trim().to_string()
    };
    if actual_bus_address.is_empty() {
        return Err("dbus-daemon did not print its address (crashed?)".to_string());
    }

    let secretsd_pid = unsafe { libc::fork() };
    if secretsd_pid < 0 {
        return Err(format!(
            "fork secretsd failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if secretsd_pid == 0 {
        run_secretsd_child(
            SecretsdChildEndpoints {
                exec_gate_fd,
                ready_read,
                ready_write,
                init_read,
                init_write,
                publisher,
            },
            |selected| {
                use std::os::unix::process::CommandExt;
                let init_fd = selected.init_read.as_raw_fd();
                let publication_fd = selected.publisher.as_raw_fd();
                let ready_fd = selected.ready_write.as_raw_fd();
                let mut secretsd_cmd = Command::new(&secretsd);
                secretsd_cmd
                    .arg("--bus-address")
                    .arg(&actual_bus_address)
                    .arg("--init-fd")
                    .arg(init_fd.to_string())
                    .arg("--publication-fd")
                    .arg(publication_fd.to_string())
                    .arg("--ready-fd")
                    .arg(ready_fd.to_string())
                    .stderr({
                        let log = std::fs::File::create(format!("{bus_dir}/secretsd.log"))
                            .unwrap_or_else(|_| std::fs::File::open("/dev/null").unwrap());
                        std::process::Stdio::from(log)
                    });
                apply_helper_env(&mut secretsd_cmd, helper_env);
                let error = secretsd_cmd.exec();
                Err(format!("exec secretsd failed: {error}"))
            },
        );
    }

    // PID-2 setup retains no publisher after the secretsd fork, before waiting
    // for readiness or exposing any helper to the target.
    drop(publisher);
    drop(init_read);
    drop(ready_write);
    {
        let mut writer = std::fs::File::from(init_write);
        writer
            .write_all(section_bytes)
            .map_err(|e| format!("writing libsecret section to init pipe: {e}"))?;
        // Dropping the writer closes the pipe so secretsd sees EOF.
    }

    let mut ready_buf = [0u8; 1];
    loop {
        let n = unsafe {
            libc::read(
                ready_read.as_raw_fd(),
                ready_buf.as_mut_ptr() as *mut libc::c_void,
                1,
            )
        };
        if n > 0 {
            break;
        }
        if n == 0 {
            return Err("secretsd exited before signaling readiness".to_string());
        }
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(format!("reading secretsd readiness: {err}"));
    }
    drop(ready_read);

    cmd.env("DBUS_SESSION_BUS_ADDRESS", &actual_bus_address);

    Ok(())
}

#[cfg(test)]
mod tests;
