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
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
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

fn set_cloexec(fd: RawFd, cloexec: bool) -> Result<(), String> {
    // SAFETY: F_GETFD reads flags from the live descriptor named by fd.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
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
        unsafe {
            libc::close(exec_gate_fd);
            libc::close(ready_read.as_raw_fd());
            libc::close(ready_write.as_raw_fd());
            libc::close(dbus_addr_read.as_raw_fd());
            libc::close(init_read.as_raw_fd());
            libc::close(init_write.as_raw_fd());
            libc::close(publisher.as_raw_fd());
        }
        if let Err(error) = set_cloexec(dbus_addr_write.as_raw_fd(), false) {
            eprintln!("underwrap-ns: preparing dbus address fd: {error}");
            std::process::exit(1);
        }
        use std::os::unix::process::CommandExt;
        let mut dbus_cmd = Command::new(&dbus_daemon);
        dbus_cmd
            .arg("--session")
            .arg(format!("--address={bus_address}"))
            .arg("--nofork")
            .arg("--nopidfile")
            .arg(format!("--print-address={}", dbus_addr_write.as_raw_fd()))
            .stderr({
                let log = std::fs::File::create(format!("{bus_dir}/dbus-daemon.log"))
                    .unwrap_or_else(|_| std::fs::File::open("/dev/null").unwrap());
                std::process::Stdio::from(log)
            });
        apply_helper_env(&mut dbus_cmd, helper_env);
        let err = dbus_cmd.exec();
        eprintln!("underwrap-ns: exec dbus-daemon failed: {err}");
        std::process::exit(1);
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
        unsafe {
            libc::close(exec_gate_fd);
            libc::close(ready_read.as_raw_fd());
            libc::close(init_write.as_raw_fd());
        }
        for fd in [
            init_read.as_raw_fd(),
            ready_write.as_raw_fd(),
            publisher.as_raw_fd(),
        ] {
            if let Err(error) = set_cloexec(fd, false) {
                eprintln!("underwrap-ns: preparing secretsd inherited fd {fd}: {error}");
                std::process::exit(1);
            }
        }
        use std::os::unix::process::CommandExt;
        let mut secretsd_cmd = Command::new(&secretsd);
        secretsd_cmd
            .arg("--bus-address")
            .arg(&actual_bus_address)
            .arg("--init-fd")
            .arg(init_read.as_raw_fd().to_string())
            .arg("--publication-fd")
            .arg(publisher.as_raw_fd().to_string())
            .arg("--ready-fd")
            .arg(ready_write.as_raw_fd().to_string())
            .stderr({
                let log = std::fs::File::create(format!("{bus_dir}/secretsd.log"))
                    .unwrap_or_else(|_| std::fs::File::open("/dev/null").unwrap());
                std::process::Stdio::from(log)
            });
        apply_helper_env(&mut secretsd_cmd, helper_env);
        let err = secretsd_cmd.exec();
        eprintln!("underwrap-ns: exec secretsd failed: {err}");
        std::process::exit(1);
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
