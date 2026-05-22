//! Boot a private dbus-daemon and `underwrap-secretsd` inside the
//! namespace and point the target at the private bus.
//!
//! The libsecret keyring for this run is provided as JCS-canonical
//! section bytes (loaded by ns from the resource blob's `libsecret`
//! section at Checkout time). ns writes those bytes into a one-shot
//! pipe and passes the read end as `--init-fd` to secretsd; secretsd
//! holds the section entirely in memory and persists each D-Bus
//! mutation atomically to `--state-path` so ns can read the final
//! state after the namespace dies.

use std::io::Write;
use std::path::Path;
use std::process::Command;

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

pub(crate) fn setup_libsecret(
    cmd: &mut Command,
    secret_fd: i32,
    section_bytes: &[u8],
    state_path: &Path,
) -> Result<(), String> {
    let dbus_daemon = require_absolute_env_path("UNDERWRAP_DBUS_DAEMON")?;
    let secretsd = require_absolute_env_path("UNDERWRAP_SECRETSD")?;

    let bus_dir = "/tmp/.underwrap-ns-bus";
    let bus_socket = format!("{bus_dir}/bus");
    let bus_address = format!("unix:path={bus_socket}");

    let mut ready_fds = [0i32; 2];
    let ret = unsafe { libc::pipe(ready_fds.as_mut_ptr()) };
    if ret != 0 {
        return Err(format!(
            "pipe() for readiness failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let (ready_read, ready_write) = (ready_fds[0], ready_fds[1]);

    let mut dbus_addr_fds = [0i32; 2];
    let ret = unsafe { libc::pipe(dbus_addr_fds.as_mut_ptr()) };
    if ret != 0 {
        return Err(format!(
            "pipe() for dbus address failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let (dbus_addr_read, dbus_addr_write) = (dbus_addr_fds[0], dbus_addr_fds[1]);

    // One-shot init pipe carrying the libsecret section bytes. Writer
    // closes after writing so secretsd sees EOF on the read end.
    let mut init_fds = [0i32; 2];
    let ret = unsafe { libc::pipe(init_fds.as_mut_ptr()) };
    if ret != 0 {
        return Err(format!(
            "pipe() for libsecret init failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let (init_read, init_write) = (init_fds[0], init_fds[1]);

    let dbus_pid = unsafe { libc::fork() };
    if dbus_pid < 0 {
        return Err(format!(
            "fork dbus-daemon failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if dbus_pid == 0 {
        unsafe {
            libc::close(secret_fd);
            libc::close(ready_read);
            libc::close(ready_write);
            libc::close(dbus_addr_read);
            libc::close(init_read);
            libc::close(init_write);
        }
        use std::os::unix::process::CommandExt;
        let err = Command::new(&dbus_daemon)
            .arg("--session")
            .arg(format!("--address={bus_address}"))
            .arg("--nofork")
            .arg("--nopidfile")
            .arg(format!("--print-address={dbus_addr_write}"))
            .stderr({
                let log = std::fs::File::create(format!("{bus_dir}/dbus-daemon.log"))
                    .unwrap_or_else(|_| std::fs::File::open("/dev/null").unwrap());
                std::process::Stdio::from(log)
            })
            .exec();
        eprintln!("underwrap-ns: exec dbus-daemon failed: {err}");
        std::process::exit(1);
    }

    unsafe { libc::close(dbus_addr_write) };

    let actual_bus_address = {
        use std::io::BufRead;
        use std::os::unix::io::FromRawFd;
        let file = unsafe { std::fs::File::from_raw_fd(dbus_addr_read) };
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

    let state_path_str = state_path
        .to_str()
        .ok_or_else(|| format!("--state-path {} is not valid UTF-8", state_path.display()))?
        .to_string();

    let secretsd_pid = unsafe { libc::fork() };
    if secretsd_pid < 0 {
        return Err(format!(
            "fork secretsd failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if secretsd_pid == 0 {
        unsafe {
            libc::close(secret_fd);
            libc::close(ready_read);
            libc::close(init_write);
        }
        use std::os::unix::process::CommandExt;
        let err = Command::new(&secretsd)
            .arg("--bus-address")
            .arg(&actual_bus_address)
            .arg("--init-fd")
            .arg(init_read.to_string())
            .arg("--state-path")
            .arg(&state_path_str)
            .arg("--ready-fd")
            .arg(ready_write.to_string())
            .stderr({
                let log = std::fs::File::create(format!("{bus_dir}/secretsd.log"))
                    .unwrap_or_else(|_| std::fs::File::open("/dev/null").unwrap());
                std::process::Stdio::from(log)
            })
            .exec();
        eprintln!("underwrap-ns: exec secretsd failed: {err}");
        std::process::exit(1);
    }

    // Parent (ns): close fds the child owns and feed the init bytes.
    unsafe {
        libc::close(init_read);
        libc::close(ready_write);
    }
    {
        use std::os::unix::io::FromRawFd;
        let mut writer = unsafe { std::fs::File::from_raw_fd(init_write) };
        writer
            .write_all(section_bytes)
            .map_err(|e| format!("writing libsecret section to init pipe: {e}"))?;
        // Dropping the writer closes the pipe so secretsd sees EOF.
    }

    let mut ready_buf = [0u8; 1];
    loop {
        let n = unsafe { libc::read(ready_read, ready_buf.as_mut_ptr() as *mut libc::c_void, 1) };
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
    unsafe { libc::close(ready_read) };

    cmd.env("DBUS_SESSION_BUS_ADDRESS", &actual_bus_address);

    Ok(())
}

#[cfg(test)]
mod tests;
