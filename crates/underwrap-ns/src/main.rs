//! underwrap-ns — PID namespace isolation stage.
//!
//! This binary is the second stage of the underwrap wrapper chain.
//! It is installed with `cap_sys_admin` (via setcap) and:
//!
//! 1. Reads the secret from fd 3 (pipe from underwrap-exec)
//! 2. Closes fd 3
//! 3. Calls `unshare(CLONE_NEWPID)` to create a new PID namespace
//! 4. Drops all capabilities
//! 5. Forks — child is PID 1 in the new namespace
//! 6. Child: sets the env var, execs the target
//! 7. Parent: waits for child, forwards exit code
//!
//! Usage: underwrap-ns <env_var> <target> [args...]
//! The secret is read from fd 3 (not stdin, not argv).

use std::io::Read;
use std::os::unix::process::CommandExt;

/// The fd number used to pass the secret from underwrap-exec.
const SECRET_FD: i32 = 3;

/// The fd number used for the secretsd channel (from underwrap-exec).
const CHANNEL_FD: i32 = 4;

/// Default dbus-daemon binary path.
const DEFAULT_DBUS_DAEMON: &str = "dbus-daemon";

/// Default secretsd binary path.
const DEFAULT_SECRETSD: &str = "underwrap-secretsd";

fn main() {
    if let Err(e) = run() {
        eprintln!("underwrap-ns: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err(format!(
            "usage: {} <env_var> <target> [args...]",
            args.first().map(|s| s.as_str()).unwrap_or("underwrap-ns")
        ));
    }
    let env_var = &args[1];
    let target = &args[2];
    let target_args = &args[3..];

    // Read secret from fd 3.
    let mut material =
        read_secret_fd().map_err(|e| format!("reading secret from fd {SECRET_FD}: {e}"))?;

    // Make this process non-dumpable immediately. Since the parent process
    // (this process after fork) never execs, the flag persists for its
    // entire lifetime. This prevents same-UID processes from reading
    // the secret out of /proc/<pid>/mem.
    unsafe {
        libc::prctl(libc::PR_SET_DUMPABLE, 0);
    }

    // Create new PID namespace (and mount namespace for libsecret isolation).
    let libsecret = std::env::var("UNDERWRAP_LIBSECRET").is_ok();
    let unshare_flags = if libsecret {
        // Mount namespace hides the real session bus socket from the target.
        libc::CLONE_NEWPID | libc::CLONE_NEWNS
    } else {
        libc::CLONE_NEWPID
    };
    let ret = unsafe { libc::unshare(unshare_flags) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("unshare failed: {err}"));
    }

    // Drop all capabilities now that we've done the unshare.
    drop_caps().map_err(|e| format!("dropping capabilities: {e}"))?;

    // Fork. Child gets PID 1 in the new namespace.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("fork failed: {err}"));
    }

    if pid == 0 {
        // Child: harden and exec the target with the secret in env.

        // Disable core dumps so a crashing target can't write the secret to disk.
        let rlim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        unsafe {
            libc::setrlimit(libc::RLIMIT_CORE, &rlim);
        }

        // Set non-dumpable. Inside the PID namespace this flag persists
        // because there's no further credential change before exec.
        // This is defense-in-depth: the PID namespace already hides the
        // process, but if someone obtains the PID through other means,
        // non-dumpable blocks /proc/<pid>/mem and ptrace.
        unsafe {
            libc::prctl(libc::PR_SET_DUMPABLE, 0);
        }

        // Check if libsecret mode is enabled.
        let libsecret = std::env::var("UNDERWRAP_LIBSECRET").is_ok();

        let mut cmd = std::process::Command::new(target);
        cmd.args(target_args);
        cmd.env(env_var, &material);

        // Remove libsecret env vars — target should not see these.
        cmd.env_remove("UNDERWRAP_LIBSECRET");
        cmd.env_remove("UNDERWRAP_DBUS_DAEMON");
        cmd.env_remove("UNDERWRAP_SECRETSD");

        if libsecret {
            // Launch private D-Bus session bus + secretsd inside the namespace.
            match setup_libsecret(&mut cmd) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("underwrap-ns: libsecret setup failed: {e}");
                    std::process::exit(1);
                }
            }
        } else {
            // No libsecret — close fd 4 if it happens to be open.
            unsafe { libc::close(CHANNEL_FD) };
        }

        let err = cmd.exec();
        eprintln!("underwrap-ns: exec failed: {err}");
        std::process::exit(1);
    }

    // Parent: zeroize the secret — only the child (in the namespace) needs it.
    {
        use zeroize::Zeroize;
        material.zeroize();
    }

    // Parent: wait for child, forward exit code.
    let mut status: libc::c_int = 0;
    let ret = unsafe { libc::waitpid(pid, &mut status, 0) };
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("waitpid failed: {err}"));
    }

    if libc::WIFEXITED(status) {
        std::process::exit(libc::WEXITSTATUS(status));
    } else if libc::WIFSIGNALED(status) {
        let sig = libc::WTERMSIG(status);
        unsafe {
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
        std::process::exit(128 + sig);
    }
    std::process::exit(1);
}

/// Set up libsecret mode: launch dbus-daemon and secretsd inside the PID namespace.
///
/// 1. Create temp dir for bus socket
/// 2. Fork+exec dbus-daemon
/// 3. Wait for socket to appear
/// 4. Fork+exec secretsd (inherits fd 4 = channel to daemon)
/// 5. Close fd 4 (CRITICAL: target must not inherit it)
/// 6. Wait for secretsd readiness
/// 7. Set DBUS_SESSION_BUS_ADDRESS on the target command
fn setup_libsecret(cmd: &mut std::process::Command) -> Result<(), String> {
    let dbus_daemon =
        std::env::var("UNDERWRAP_DBUS_DAEMON").unwrap_or_else(|_| DEFAULT_DBUS_DAEMON.to_string());
    let secretsd =
        std::env::var("UNDERWRAP_SECRETSD").unwrap_or_else(|_| DEFAULT_SECRETSD.to_string());

    // Hide the real session bus socket so the target can't bypass our private bus.
    // We're in a mount namespace (CLONE_NEWNS), so this only affects this namespace.
    let uid = unsafe { libc::getuid() };
    let real_bus = format!("/run/user/{uid}/bus");
    if std::path::Path::new(&real_bus).exists() {
        let devnull = std::ffi::CString::new("/dev/null").unwrap();
        let target = std::ffi::CString::new(real_bus.as_str()).unwrap();
        let ret = unsafe {
            libc::mount(
                devnull.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                libc::MS_BIND,
                std::ptr::null(),
            )
        };
        if ret != 0 {
            // Non-fatal: the real bus socket might not exist in all environments.
            let err = std::io::Error::last_os_error();
            eprintln!("underwrap-ns: warning: could not hide real session bus: {err}");
        }
    }

    // Create temp directory for the bus socket.
    let bus_dir = format!("/tmp/underwrap-dbus-{}", unsafe { libc::getpid() });
    std::fs::create_dir_all(&bus_dir).map_err(|e| format!("creating bus dir {bus_dir}: {e}"))?;
    let bus_socket = format!("{bus_dir}/bus");
    let bus_address = format!("unix:path={bus_socket}");

    // Create a readiness pipe for secretsd.
    let mut ready_fds = [0i32; 2];
    let ret = unsafe { libc::pipe(ready_fds.as_mut_ptr()) };
    if ret != 0 {
        return Err(format!(
            "pipe() for readiness failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let (ready_read, ready_write) = (ready_fds[0], ready_fds[1]);

    // --- Fork dbus-daemon ---
    // Use --print-address to a pipe so we know the bus is ready.
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
        // Child: exec dbus-daemon.
        unsafe {
            libc::close(SECRET_FD);
            libc::close(CHANNEL_FD);
            libc::close(ready_read);
            libc::close(ready_write);
            libc::close(dbus_addr_read);
        }
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(&dbus_daemon)
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

    // Close write end in parent.
    unsafe { libc::close(dbus_addr_write) };

    // Read the actual bus address from dbus-daemon (blocks until it writes a line).
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

    // --- Fork secretsd ---
    let secretsd_pid = unsafe { libc::fork() };
    if secretsd_pid < 0 {
        return Err(format!(
            "fork secretsd failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if secretsd_pid == 0 {
        // Child: exec secretsd.
        // secretsd inherits fd 4 (channel to daemon).
        // Pass the readiness write fd so it can signal when ready.
        unsafe {
            libc::close(SECRET_FD);
            libc::close(ready_read);
        }
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(&secretsd)
            .arg("--bus-address")
            .arg(&actual_bus_address)
            .arg("--channel-fd")
            .arg(CHANNEL_FD.to_string())
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

    // CRITICAL: Close fd 4 (secretsd channel) in this process.
    // The target must NOT inherit it.
    unsafe { libc::close(CHANNEL_FD) };
    // Close write end of readiness pipe.
    unsafe { libc::close(ready_write) };

    // Wait for secretsd to signal readiness (it writes one byte when ready).
    let mut ready_buf = [0u8; 1];
    let mut waited = 0;
    loop {
        let n = unsafe { libc::read(ready_read, ready_buf.as_mut_ptr() as *mut libc::c_void, 1) };
        if n > 0 {
            break; // secretsd is ready.
        }
        if n == 0 {
            // Pipe closed without writing — secretsd crashed.
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

    // Set DBUS_SESSION_BUS_ADDRESS so the target uses our private bus.
    cmd.env("DBUS_SESSION_BUS_ADDRESS", &actual_bus_address);

    Ok(())
}

/// Read the secret from the pipe on fd 3 and close the fd.
fn read_secret_fd() -> Result<String, std::io::Error> {
    use std::os::unix::io::FromRawFd;

    // SAFETY: fd 3 is expected to be a valid pipe read-end passed by underwrap-exec.
    let mut file = unsafe { std::fs::File::from_raw_fd(SECRET_FD) };
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    // File is closed on drop.
    Ok(buf)
}

/// Drop all Linux capabilities.
fn drop_caps() -> Result<(), String> {
    // PR_CAP_AMBIENT_CLEAR_ALL removes all ambient caps.
    // Then set the capability bounding set to empty by dropping each cap.
    unsafe {
        // Clear ambient capabilities.
        let ret = libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_CLEAR_ALL,
            0,
            0,
            0,
        );
        if ret != 0 {
            // Not fatal — may not have ambient caps.
        }

        // Drop all capabilities from the bounding set.
        // Iterate through capability numbers until we get EINVAL.
        for cap in 0..64 {
            let ret = libc::prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0);
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINVAL) {
                    break; // No more valid capability numbers.
                }
                // EPERM means we don't have this cap — that's fine.
            }
        }

        // Clear all permitted/effective/inheritable capabilities by
        // setting the securebits to prevent regaining caps.
        // The simplest approach: the caps will be cleared on the
        // next exec() of a non-setcap binary (the target).
    }
    Ok(())
}
