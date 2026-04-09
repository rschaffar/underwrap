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

mod caps;
mod directory_credential;
mod env;
mod libsecret;
mod mounts;

use std::os::unix::process::CommandExt;
use std::path::PathBuf;

use caps::{drop_caps, set_no_new_privs};
use directory_credential::{
    cleanup_directory_mounts, close_directory_channel_fds_for_exec, prepare_directory_mounts,
    sync_directory_mounts,
};
use env::build_target_env;
use libsecret::setup_libsecret;
use mounts::{
    cleanup_directory_staging_root, setup_directory_mount_namespace_base, setup_mount_namespace,
};
use underwrap_protocol::exec::ExecBundle;

/// The fd number used to pass the secret from underwrap-exec.
const SECRET_FD: i32 = 3;

/// The fd number used for the secretsd channel (from underwrap-exec).
const CHANNEL_FD: i32 = 4;
pub(crate) const DIRECTORY_STAGING_ROOT: &str = "/tmp/.underwrap-ns-directories";

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

    // Read the execution bundle from fd 3.
    let exec_bundle =
        read_secret_fd().map_err(|e| format!("reading exec bundle from fd {SECRET_FD}: {e}"))?;

    let has_directory_credentials = !exec_bundle.directories.is_empty();
    let target_home =
        resolve_target_home().map_err(|e| format!("resolving target home directory: {e}"))?;

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

    // Create new PID namespace and, when needed, a private mount namespace.
    let libsecret = std::env::var("UNDERWRAP_LIBSECRET").is_ok();
    let unshare_flags = if libsecret || has_directory_credentials {
        libc::CLONE_NEWPID | libc::CLONE_NEWNS
    } else {
        libc::CLONE_NEWPID
    };
    let ret = unsafe { libc::unshare(unshare_flags) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("unshare failed: {err}"));
    }

    if libsecret || has_directory_credentials {
        setup_directory_mount_namespace_base(DIRECTORY_STAGING_ROOT, has_directory_credentials)
            .map_err(|e| format!("mount namespace setup failed: {e}"))?;
    }

    let mounted_directories = if has_directory_credentials {
        prepare_directory_mounts(
            &exec_bundle.directories,
            &target_home,
            &PathBuf::from(DIRECTORY_STAGING_ROOT),
        )
        .map_err(|e| format!("preparing directory credentials: {e}"))?
    } else {
        Vec::new()
    };

    // Do NOT mount /proc here — we're still in the parent PID namespace.
    // unshare(CLONE_NEWPID) only affects the next fork's child.

    // Do NOT drop caps here — the child still needs cap_sys_admin for
    // libsecret-specific mount operations.

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

        // Prevent the target and its descendants from gaining privilege via
        // setuid/setgid or file capabilities on later execs.
        set_no_new_privs().map_err(|e| format!("setting no_new_privs: {e}"))?;

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
        let host_env: std::collections::HashMap<String, String> = std::env::vars().collect();
        let target_env = build_target_env(&host_env, &exec_bundle)?;
        for (key, value) in target_env {
            cmd.env(key, value);
        }

        if libsecret {
            // Launch private D-Bus session bus + secretsd inside the namespace.
            match setup_libsecret(&mut cmd, SECRET_FD, CHANNEL_FD) {
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

        // Directory channels are only for wrapper-controlled staging/sync.
        // The final target must not inherit them.
        close_directory_channel_fds_for_exec(&mounted_directories);

        let err = cmd.exec();
        eprintln!("underwrap-ns: exec failed: {err}");
        std::process::exit(1);
    }

    // Parent: if there are no directory credentials, it no longer needs
    // cap_sys_admin after the fork. Directory cleanup requires it later.
    if !has_directory_credentials {
        drop_caps().map_err(|e| format!("parent: dropping capabilities: {e}"))?;
    }

    // Parent: wait for child, then sync and clean up directory mounts.
    let mut status: libc::c_int = 0;
    let ret = unsafe { libc::waitpid(pid, &mut status, 0) };
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("waitpid failed: {err}"));
    }

    let sync_result = sync_directory_mounts(&mounted_directories);
    let cleanup_result = cleanup_directory_mounts(&mounted_directories);
    let staging_cleanup_result = if has_directory_credentials {
        cleanup_directory_staging_root(DIRECTORY_STAGING_ROOT)
    } else {
        Ok(())
    };

    if let Err(e) = cleanup_result {
        return Err(format!("directory cleanup failed: {e}"));
    }
    if let Err(e) = staging_cleanup_result {
        return Err(format!("directory staging cleanup failed: {e}"));
    }
    if let Err(e) = sync_result {
        return Err(format!("directory sync failed: {e}"));
    }

    if has_directory_credentials {
        drop_caps().map_err(|e| format!("parent: dropping capabilities: {e}"))?;
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

fn resolve_target_home() -> Result<PathBuf, std::io::Error> {
    let uid = unsafe { libc::getuid() };
    let pwd = unsafe { libc::getpwuid(uid) };
    if pwd.is_null() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("getpwuid({uid}) returned null"),
        ));
    }
    let home = unsafe { std::ffi::CStr::from_ptr((*pwd).pw_dir) }
        .to_str()
        .map_err(std::io::Error::other)?;
    Ok(PathBuf::from(home))
}

/// Read the execution bundle from the pipe on fd 3 and close the fd.
fn read_secret_fd() -> Result<ExecBundle, std::io::Error> {
    use std::os::unix::io::FromRawFd;

    // SAFETY: fd 3 is expected to be a valid pipe read-end passed by underwrap-exec.
    let mut file = unsafe { std::fs::File::from_raw_fd(SECRET_FD) };
    underwrap_protocol::ipc::read_frame_sync(&mut file).map_err(std::io::Error::other)
}
