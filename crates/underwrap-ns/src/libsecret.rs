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
    channel_fd: i32,
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
            libc::close(channel_fd);
            libc::close(ready_read);
            libc::close(ready_write);
            libc::close(dbus_addr_read);
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
        }
        use std::os::unix::process::CommandExt;
        let err = Command::new(&secretsd)
            .arg("--bus-address")
            .arg(&actual_bus_address)
            .arg("--channel-fd")
            .arg(channel_fd.to_string())
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

    unsafe { libc::close(channel_fd) };
    unsafe { libc::close(ready_write) };

    let mut ready_buf = [0u8; 1];
    let mut waited = 0;
    loop {
        let n = unsafe { libc::read(ready_read, ready_buf.as_mut_ptr() as *mut libc::c_void, 1) };
        if n > 0 {
            break;
        }
        if n == 0 {
            return Err("secretsd exited before signaling readiness".to_string());
        }
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("reading secretsd readiness: {err}"));
        }
        waited += 1;
        if waited > 50 {
            return Err("secretsd did not become ready within 5 seconds".to_string());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    unsafe { libc::close(ready_read) };

    cmd.env("DBUS_SESSION_BUS_ADDRESS", &actual_bus_address);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
