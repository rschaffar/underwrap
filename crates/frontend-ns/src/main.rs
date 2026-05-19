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
mod resource_channel;
mod writeback;

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

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
    let (target, target_args) = parse_target_invocation(std::env::args().collect())?;
    let exec_bundle =
        read_secret_fd().map_err(|e| format!("reading exec bundle from fd {SECRET_FD}: {e}"))?;

    let has_directory_credentials = !exec_bundle.directories.is_empty();
    let libsecret_enabled = std::env::var("UNDERWRAP_LIBSECRET").is_ok();
    let target_home =
        resolve_target_home().map_err(|e| format!("resolving target home directory: {e}"))?;

    make_process_non_dumpable()?;
    prepare_namespace(libsecret_enabled, has_directory_credentials)?;

    let mounted_directories =
        prepare_mounted_directories(&exec_bundle, has_directory_credentials, &target_home)?;

    // Capture per-resource Checkout state for the end-of-run
    // writeback decision. No-op when the daemon hasn't allocated
    // any wrapper-resource channels yet.
    let resource_channels = resource_channel::checkout_all(&exec_bundle.resources)?;

    let pid = fork_into_namespace()?;
    if pid == 0 {
        run_child(
            target,
            target_args,
            &exec_bundle,
            &mounted_directories,
            &resource_channels,
            libsecret_enabled,
        )
    } else {
        run_parent(
            pid,
            &mounted_directories,
            &resource_channels,
            has_directory_credentials,
        )
    }
}

fn parse_target_invocation(args: Vec<String>) -> Result<(String, Vec<String>), String> {
    if args.len() < 2 {
        return Err(format!(
            "usage: {} <target> [args...]",
            args.first().map(|s| s.as_str()).unwrap_or("underwrap-ns")
        ));
    }
    Ok((args[1].clone(), args[2..].to_vec()))
}

fn make_process_non_dumpable() -> Result<(), String> {
    let ret = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) };
    if ret != 0 {
        return Err(format!(
            "prctl(PR_SET_DUMPABLE, 0) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn prepare_namespace(
    libsecret_enabled: bool,
    has_directory_credentials: bool,
) -> Result<(), String> {
    let unshare_flags = if libsecret_enabled || has_directory_credentials {
        libc::CLONE_NEWPID | libc::CLONE_NEWNS
    } else {
        libc::CLONE_NEWPID
    };
    let ret = unsafe { libc::unshare(unshare_flags) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("unshare failed: {err}"));
    }

    if libsecret_enabled || has_directory_credentials {
        setup_directory_mount_namespace_base(DIRECTORY_STAGING_ROOT, has_directory_credentials)
            .map_err(|e| format!("mount namespace setup failed: {e}"))?;
    }

    Ok(())
}

fn prepare_mounted_directories(
    exec_bundle: &ExecBundle,
    has_directory_credentials: bool,
    target_home: &Path,
) -> Result<Vec<directory_credential::MountedDirectory>, String> {
    if has_directory_credentials {
        prepare_directory_mounts(
            &exec_bundle.directories,
            target_home,
            &PathBuf::from(DIRECTORY_STAGING_ROOT),
        )
        .map_err(|e| format!("preparing directory credentials: {e}"))
    } else {
        Ok(Vec::new())
    }
}

fn fork_into_namespace() -> Result<libc::pid_t, String> {
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("fork failed: {err}"));
    }
    Ok(pid)
}

fn run_child(
    target: String,
    target_args: Vec<String>,
    exec_bundle: &ExecBundle,
    mounted_directories: &[directory_credential::MountedDirectory],
    resource_channels: &[resource_channel::ResourceChannel],
    libsecret_enabled: bool,
) -> Result<(), String> {
    if libsecret_enabled {
        setup_mount_namespace().map_err(|e| format!("mount namespace setup failed: {e}"))?;
    }

    set_no_new_privs().map_err(|e| format!("setting no_new_privs: {e}"))?;
    drop_caps().map_err(|e| format!("dropping capabilities: {e}"))?;
    disable_core_dumps()?;
    make_process_non_dumpable()?;

    let mut cmd = std::process::Command::new(&target);
    cmd.args(&target_args);
    cmd.env_clear();

    let host_env: std::collections::HashMap<String, String> = std::env::vars().collect();
    let target_env = build_target_env(&host_env, exec_bundle)?;
    for (key, value) in target_env {
        cmd.env(key, value);
    }

    if libsecret_enabled {
        setup_libsecret(&mut cmd, SECRET_FD, CHANNEL_FD)?;
    } else {
        unsafe { libc::close(CHANNEL_FD) };
    }

    close_directory_channel_fds_for_exec(mounted_directories);
    resource_channel::close_channel_fds_for_exec(resource_channels);

    let err = cmd.exec();
    Err(format!("exec failed: {err}"))
}

fn disable_core_dumps() -> Result<(), String> {
    let rlim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    let ret = unsafe { libc::setrlimit(libc::RLIMIT_CORE, &rlim) };
    if ret != 0 {
        return Err(format!(
            "setrlimit(RLIMIT_CORE, 0) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn run_parent(
    pid: libc::pid_t,
    mounted_directories: &[directory_credential::MountedDirectory],
    resource_channels: &[resource_channel::ResourceChannel],
    has_directory_credentials: bool,
) -> Result<(), String> {
    if !has_directory_credentials {
        drop_caps().map_err(|e| format!("parent: dropping capabilities: {e}"))?;
    }

    let status = wait_for_child(pid)?;
    sync_and_cleanup_directories(mounted_directories, has_directory_credentials)?;

    // Best-effort UsageSignal on each wrapper-resource channel.
    // Errors here must not mask a target failure, so they go to
    // stderr without changing the exit status.
    if let Err(e) = writeback::signal_usage_for_all(resource_channels) {
        eprintln!("underwrap-ns: end-of-run UsageSignal dispatch: {e}");
    }

    if has_directory_credentials {
        drop_caps().map_err(|e| format!("parent: dropping capabilities: {e}"))?;
    }

    propagate_exit_status(status)
}

fn wait_for_child(pid: libc::pid_t) -> Result<libc::c_int, String> {
    let mut status: libc::c_int = 0;
    let ret = unsafe { libc::waitpid(pid, &mut status, 0) };
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("waitpid failed: {err}"));
    }
    Ok(status)
}

fn sync_and_cleanup_directories(
    mounted_directories: &[directory_credential::MountedDirectory],
    has_directory_credentials: bool,
) -> Result<(), String> {
    let sync_result = sync_directory_mounts(mounted_directories);
    let cleanup_result = cleanup_directory_mounts(mounted_directories);
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
    match sync_result {
        Ok(outcomes) => report_sync_outcomes(&outcomes),
        Err(e) => return Err(format!("directory sync failed: {e}")),
    }

    Ok(())
}

/// `UNDERWRAP_SUPPRESS_READONLY_WARNING=1` silences the stderr line
/// emitted when the backend rejects a directory writeback. The
/// structured journal event is always logged.
const SUPPRESS_READONLY_WARNING_ENV: &str = "UNDERWRAP_SUPPRESS_READONLY_WARNING";

fn read_only_warning_suppressed() -> bool {
    matches!(
        std::env::var(SUPPRESS_READONLY_WARNING_ENV).as_deref(),
        Ok("1")
    )
}

fn report_sync_outcomes(outcomes: &[directory_credential::SyncOutcome]) {
    let suppress = read_only_warning_suppressed();
    for outcome in outcomes {
        if let directory_credential::SyncOutcome::Rejected { resource_id } = outcome {
            // Structured journal event: always emitted.
            eprintln_journal_event(resource_id);
            // Human-readable stderr line: suppressible.
            if !suppress {
                eprintln!(
                    "underwrap: changes to resource '{resource_id}' were not \
                     persisted (grant is read-only). Set \
                     {SUPPRESS_READONLY_WARNING_ENV}=1 to silence this message.",
                );
            }
        }
    }
}

/// Emit the structured journal event. Uses tracing so the systemd
/// journal sees `audit=writeback_rejected reason=grant_readonly
/// resource=<id>` on the wrapper-ns process.
fn eprintln_journal_event(resource_id: &str) {
    // tracing-subscriber isn't installed in frontend-ns at the moment
    // (no tracing init here), so the structured form lands on
    // stderr-by-default. journald captures stderr from systemd units,
    // which is the operational case. The line shape mirrors the
    // audit events emitted by underwrapd.
    eprintln!(
        r#"{{"audit":"writeback_rejected","reason":"grant_readonly","resource":"{resource_id}"}}"#,
    );
}

fn propagate_exit_status(status: libc::c_int) -> Result<(), String> {
    if libc::WIFEXITED(status) {
        std::process::exit(libc::WEXITSTATUS(status));
    }
    if libc::WIFSIGNALED(status) {
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
