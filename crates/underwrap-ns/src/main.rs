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
//! Usage: underwrap-ns <target> [args...]
//! Environment secret bindings are read from fd 3 (not stdin, not argv).

use std::io::Read;
use std::os::unix::process::CommandExt;

/// The fd number used to pass the secret from underwrap-exec.
const SECRET_FD: i32 = 3;

/// The fd number used for the secretsd channel (from underwrap-exec).
const CHANNEL_FD: i32 = 4;

fn main() {
    if let Err(e) = run() {
        eprintln!("underwrap-ns: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        return Err(format!(
            "usage: {} <target> [args...]",
            args.first().map(|s| s.as_str()).unwrap_or("underwrap-ns")
        ));
    }
    let target = &args[1];
    let target_args = &args[2..];

    // Read env secret bindings from fd 3.
    let env_secrets =
        read_secret_fd().map_err(|e| format!("reading env bindings from fd {SECRET_FD}: {e}"))?;

    // Make this process non-dumpable immediately. Since the parent process
    // (this process after fork) never execs, the flag persists for its
    // entire lifetime. This prevents same-UID processes from reading
    // the secret out of /proc/<pid>/mem.
    let ret = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) };
    if ret != 0 {
        return Err(format!(
            "prctl(PR_SET_DUMPABLE, 0) failed: {}",
            std::io::Error::last_os_error()
        ));
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

    // Do NOT mount /proc or set up mounts here — we're still in the parent
    // PID namespace. unshare(CLONE_NEWPID) only affects the next fork's child.
    // Mounting /proc here would mount the PARENT namespace's procfs.

    // Do NOT drop caps here — the child needs cap_sys_admin for mount ops.
    // Caps will be dropped in the child after mount setup.

    // Fork. Child gets PID 1 in the new namespace.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("fork failed: {err}"));
    }

    if pid == 0 {
        // Child: we are now PID 1 in the new PID namespace.

        // If libsecret mode, set up the mount namespace NOW (after fork,
        // so /proc mount reflects the new PID namespace). We still have
        // cap_sys_admin from the file capability on underwrap-ns.
        if libsecret {
            setup_mount_namespace().map_err(|e| format!("mount namespace setup failed: {e}"))?;
        }

        // Drop all capabilities now that mount setup is done.
        drop_caps().map_err(|e| format!("dropping capabilities: {e}"))?;

        // Harden and exec the target with the secret in env.

        // Disable core dumps so a crashing target can't write the secret to disk.
        let rlim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        let ret = unsafe { libc::setrlimit(libc::RLIMIT_CORE, &rlim) };
        if ret != 0 {
            eprintln!(
                "underwrap-ns: setrlimit(RLIMIT_CORE, 0) failed: {}",
                std::io::Error::last_os_error()
            );
            std::process::exit(1);
        }

        // Set non-dumpable. Inside the PID namespace this flag persists
        // because there's no further credential change before exec.
        // This is defense-in-depth: the PID namespace already hides the
        // process, but if someone obtains the PID through other means,
        // non-dumpable blocks /proc/<pid>/mem and ptrace.
        let ret = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) };
        if ret != 0 {
            eprintln!(
                "underwrap-ns: prctl(PR_SET_DUMPABLE, 0) failed: {}",
                std::io::Error::last_os_error()
            );
            std::process::exit(1);
        }

        // Check if libsecret mode is enabled.
        let libsecret = std::env::var("UNDERWRAP_LIBSECRET").is_ok();

        let mut cmd = std::process::Command::new(target);
        cmd.args(target_args);

        // === ENVIRONMENT SANITIZATION ===
        // Start with a clean environment. Only pass through vars from a
        // trusted whitelist + sidecar config. This prevents LD_PRELOAD,
        // LD_LIBRARY_PATH, and other injection vectors.
        cmd.env_clear();

        // Default safe whitelist — vars the target typically needs.
        // PATH is intentionally NOT inherited. If a wrapper needs helper
        // lookup by bare name, set PATH explicitly via trusted sidecar config.
        const DEFAULT_PASSTHROUGH: &[&str] = &[
            "HOME",
            "USER",
            "LOGNAME",
            "SHELL",
            "TERM",
            "COLORTERM",
            "LANG",
            "LC_ALL",
            "LC_COLLATE",
            "LC_CTYPE",
            "LC_MESSAGES",
            "LC_MONETARY",
            "LC_NUMERIC",
            "LC_TIME",
            "TZ",
            "XDG_RUNTIME_DIR",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
            "TMPDIR",
            "SSH_AUTH_SOCK",
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "http_proxy",
            "https_proxy",
            "no_proxy",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "NO_PROXY",
        ];

        // Pass through default whitelist.
        for var in DEFAULT_PASSTHROUGH {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }

        // Parse env config from the trusted sidecar (set by underwrap-exec).
        if let Ok(env_json) = std::env::var("UNDERWRAP_ENV_CONFIG") {
            #[derive(serde::Deserialize)]
            struct EnvConfig {
                #[serde(default)]
                passthrough: Vec<String>,
                #[serde(default)]
                set: std::collections::HashMap<String, String>,
            }
            if let Ok(cfg) = serde_json::from_str::<EnvConfig>(&env_json) {
                // Additional passthrough from sidecar config.
                for var in &cfg.passthrough {
                    if let Ok(val) = std::env::var(var) {
                        cmd.env(var, val);
                    }
                }
                // Fixed vars from sidecar config.
                for (key, val) in &cfg.set {
                    cmd.env(key, val);
                }
            }
        }

        // Inject env secret bindings.
        for (key, value) in &env_secrets {
            cmd.env(key, value);
        }

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

    // Parent: drop caps (parent doesn't need them after fork).
    drop_caps().map_err(|e| format!("parent: dropping capabilities: {e}"))?;

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

/// Set up the mount namespace for libsecret isolation.
/// MUST be called after unshare(CLONE_NEWNS) but BEFORE drop_caps(),
/// because mount operations require cap_sys_admin.
fn setup_mount_namespace() -> Result<(), String> {
    // Make all mounts private so our bind mounts don't propagate back
    // to the parent namespace.
    let root = std::ffi::CString::new("/").unwrap();
    let ret = unsafe {
        libc::mount(
            std::ptr::null(),
            root.as_ptr(),
            std::ptr::null(),
            libc::MS_PRIVATE | libc::MS_REC,
            std::ptr::null(),
        )
    };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("mount(MS_PRIVATE|MS_REC) failed: {err}"));
    }

    // Mount a fresh /proc so the PID namespace is properly isolated.
    let proc_path = std::ffi::CString::new("/proc").unwrap();
    let proc_type = std::ffi::CString::new("proc").unwrap();
    let ret = unsafe {
        libc::mount(
            proc_type.as_ptr(),
            proc_path.as_ptr(),
            proc_type.as_ptr(),
            0,
            std::ptr::null(),
        )
    };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("mount fresh /proc failed: {err}"));
    }

    // Mount a private tmpfs for the bus socket. Create the mount point
    // under /tmp (user-writable), then mount tmpfs over it. The tmpfs
    // content is only visible inside this mount namespace. We use a
    // fixed path since we're in our own mount namespace (no collision).
    let bus_dir = "/tmp/.underwrap-ns-bus";
    std::fs::create_dir_all(bus_dir)
        .map_err(|e| format!("creating bus mount point {bus_dir}: {e}"))?;
    let bus_dir_c = std::ffi::CString::new(bus_dir).unwrap();
    let tmpfs_c = std::ffi::CString::new("tmpfs").unwrap();
    let ret = unsafe {
        libc::mount(
            tmpfs_c.as_ptr(),
            bus_dir_c.as_ptr(),
            tmpfs_c.as_ptr(),
            0,
            c"size=4m,mode=0700".as_ptr() as *const libc::c_void,
        )
    };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("mount tmpfs for private bus failed: {err}"));
    }

    // Hide the real session bus socket.
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
            let err = std::io::Error::last_os_error();
            return Err(format!(
                "hiding real session bus at {real_bus} failed: {err}"
            ));
        }
    }

    Ok(())
}

/// Set up libsecret mode: launch dbus-daemon and secretsd inside the PID namespace.
/// The mount namespace (tmpfs, /proc, bus hiding) is already set up by
/// setup_mount_namespace() which ran before cap drop.
fn setup_libsecret(cmd: &mut std::process::Command) -> Result<(), String> {
    // Both binary paths MUST be absolute — no bare names, no PATH resolution.
    // They come from the trusted sidecar config via env vars set by underwrap-exec.
    let dbus_daemon = std::env::var("UNDERWRAP_DBUS_DAEMON")
        .map_err(|_| "UNDERWRAP_DBUS_DAEMON not set (required for libsecret mode)".to_string())?;
    if !dbus_daemon.starts_with('/') {
        return Err(format!(
            "UNDERWRAP_DBUS_DAEMON must be an absolute path, got: {dbus_daemon:?}"
        ));
    }
    let secretsd = std::env::var("UNDERWRAP_SECRETSD")
        .map_err(|_| "UNDERWRAP_SECRETSD not set (required for libsecret mode)".to_string())?;
    if !secretsd.starts_with('/') {
        return Err(format!(
            "UNDERWRAP_SECRETSD must be an absolute path, got: {secretsd:?}"
        ));
    }

    // The private tmpfs is already mounted by setup_mount_namespace().
    let bus_dir = "/tmp/.underwrap-ns-bus";
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

/// Read environment secret bindings from the pipe on fd 3 and close the fd.
fn read_secret_fd() -> Result<std::collections::HashMap<String, String>, std::io::Error> {
    use std::os::unix::io::FromRawFd;

    #[derive(serde::Deserialize)]
    struct SecretBundle {
        #[serde(default)]
        env: std::collections::HashMap<String, String>,
    }

    // SAFETY: fd 3 is expected to be a valid pipe read-end passed by underwrap-exec.
    let mut file = unsafe { std::fs::File::from_raw_fd(SECRET_FD) };
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    let bundle: SecretBundle = serde_json::from_str(&buf).map_err(std::io::Error::other)?;
    // File is closed on drop.
    Ok(bundle.env)
}

/// Drop all Linux capabilities.
fn drop_caps() -> Result<(), String> {
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

        // Clear effective/permitted/inheritable capability sets using capset().
        // This is the Linux capability v3 (64-bit) format.
        #[repr(C)]
        struct CapHeader {
            version: u32,
            pid: i32,
        }
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct CapData {
            effective: u32,
            permitted: u32,
            inheritable: u32,
        }
        // _LINUX_CAPABILITY_VERSION_3 = 0x20080522
        let header = CapHeader {
            version: 0x20080522,
            pid: 0, // current process
        };
        // Two data structs for 64-bit capability sets (low + high).
        let data = [CapData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];

        let ret = libc::syscall(libc::SYS_capset, &header as *const CapHeader, data.as_ptr());
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            return Err(format!("capset() failed: {err}"));
        }
    }
    Ok(())
}
