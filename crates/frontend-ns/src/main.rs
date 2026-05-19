//! underwrap-ns — PID namespace isolation stage.
//!
//! Reads the `ExecBundle` on fd 3, opens the per-resource wrapper-resource
//! channels handed to it by `underwrap-exec` (fd 20+i), drives `Checkout`
//! on each, materializes any `files` slot into a tmpfs staging dir,
//! starts a private dbus-daemon + secretsd in the namespace when a
//! libsecret resource is in scope, then forks into a fresh PID namespace
//! to exec the target.
//!
//! After waitpid, walks the staging tmpfs, reads the libsecret state
//! file secretsd wrote, composes a new `ResourceBlob` per resource and
//! dispatches the end-of-run decision (`UsageSignal` / `Checkin` /
//! drop+warn) on each channel.

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
    MountedDirectory, ResourceMountInput, cleanup_directory_mounts, prepare_directory_mounts,
};
use env::build_target_env;
use libsecret::setup_libsecret;
use mounts::{
    cleanup_directory_staging_root, setup_directory_mount_namespace_base, setup_mount_namespace,
};
use resource_channel::ResourceChannel;
use underwrap_protocol::exec::ExecBundle;
use writeback::{WritebackInput, WritebackOutcome};

/// The fd number used to pass the exec bundle from underwrap-exec.
const SECRET_FD: i32 = 3;

pub(crate) const DIRECTORY_STAGING_ROOT: &str = "/tmp/.underwrap-ns-directories";
const LIBSECRET_BUS_DIR: &str = "/tmp/.underwrap-ns-bus";
const LIBSECRET_STATE_FILENAME: &str = "libsecret-state.jcs";

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

    let libsecret_requested = std::env::var("UNDERWRAP_LIBSECRET").is_ok();
    let libsecret_resource_present = exec_bundle.resources.iter().any(|r| r.libsecret);
    let libsecret_enabled = libsecret_requested && libsecret_resource_present;
    let has_files = exec_bundle
        .resources
        .iter()
        .any(|r| !r.files_target_paths.is_empty());
    let target_home =
        resolve_target_home().map_err(|e| format!("resolving target home directory: {e}"))?;

    make_process_non_dumpable()?;
    prepare_namespace(libsecret_enabled, has_files)?;

    let resource_channels = resource_channel::checkout_all(&exec_bundle.resources)?;

    let mounted_directories =
        prepare_mounted_directories(&exec_bundle, &resource_channels, &target_home)?;

    let libsecret_state_path = if libsecret_enabled {
        Some(PathBuf::from(format!(
            "{LIBSECRET_BUS_DIR}/{LIBSECRET_STATE_FILENAME}"
        )))
    } else {
        None
    };

    let pid = fork_into_namespace()?;
    if pid == 0 {
        run_child(
            target,
            target_args,
            &exec_bundle,
            &resource_channels,
            libsecret_enabled,
            libsecret_state_path.as_deref(),
        )
    } else {
        run_parent(
            pid,
            &mounted_directories,
            &resource_channels,
            has_files,
            libsecret_state_path.as_deref(),
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

fn prepare_namespace(libsecret_enabled: bool, has_files: bool) -> Result<(), String> {
    let unshare_flags = if libsecret_enabled || has_files {
        libc::CLONE_NEWPID | libc::CLONE_NEWNS
    } else {
        libc::CLONE_NEWPID
    };
    let ret = unsafe { libc::unshare(unshare_flags) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("unshare failed: {err}"));
    }

    if libsecret_enabled || has_files {
        setup_directory_mount_namespace_base(DIRECTORY_STAGING_ROOT, has_files)
            .map_err(|e| format!("mount namespace setup failed: {e}"))?;
    }

    Ok(())
}

fn prepare_mounted_directories(
    _exec_bundle: &ExecBundle,
    resource_channels: &[ResourceChannel],
    target_home: &Path,
) -> Result<Vec<MountedDirectory>, String> {
    let mut decoded = Vec::new();
    for channel in resource_channels {
        if channel.files_target_paths.is_empty() {
            continue;
        }
        let blob = underwrap_protocol::resource_blob::ResourceBlob::from_canonical_bytes(
            &channel.original_canonical,
        )
        .map_err(|e| format!("decoding Checkout blob for '{}': {e}", channel.resource_id))?;
        decoded.push((
            channel.resource_id.clone(),
            blob,
            &channel.files_target_paths,
        ));
    }
    if decoded.is_empty() {
        return Ok(Vec::new());
    }
    let inputs: Vec<ResourceMountInput<'_>> = decoded
        .iter()
        .map(|(id, blob, paths)| ResourceMountInput {
            resource_id: id.as_str(),
            blob,
            target_paths: paths.as_slice(),
        })
        .collect();
    prepare_directory_mounts(&inputs, target_home, &PathBuf::from(DIRECTORY_STAGING_ROOT))
        .map_err(|e| format!("preparing files-slot mounts: {e}"))
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
    resource_channels: &[ResourceChannel],
    libsecret_enabled: bool,
    libsecret_state_path: Option<&Path>,
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
        let state_path = libsecret_state_path
            .ok_or_else(|| "libsecret enabled but no state path provided".to_string())?;
        let section_bytes = libsecret_section_bytes(resource_channels)?;
        setup_libsecret(&mut cmd, SECRET_FD, &section_bytes, state_path)?;
    }

    resource_channel::close_channel_fds_for_exec(resource_channels);

    let err = cmd.exec();
    Err(format!("exec failed: {err}"))
}

fn libsecret_section_bytes(channels: &[ResourceChannel]) -> Result<Vec<u8>, String> {
    let channel = channels
        .iter()
        .find(|c| c.libsecret)
        .ok_or_else(|| "libsecret enabled but no libsecret resource present".to_string())?;
    let blob = underwrap_protocol::resource_blob::ResourceBlob::from_canonical_bytes(
        &channel.original_canonical,
    )
    .map_err(|e| {
        format!(
            "decoding Checkout blob for libsecret resource '{}': {e}",
            channel.resource_id
        )
    })?;
    blob.libsecret.to_canonical_bytes().map_err(|e| {
        format!(
            "canonicalizing libsecret section for '{}': {e}",
            channel.resource_id
        )
    })
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
    mounted_directories: &[MountedDirectory],
    resource_channels: &[ResourceChannel],
    has_files: bool,
    libsecret_state_path: Option<&Path>,
) -> Result<(), String> {
    if !has_files {
        drop_caps().map_err(|e| format!("parent: dropping capabilities: {e}"))?;
    }

    let status = wait_for_child(pid)?;

    // Compose and dispatch end-of-run writes BEFORE the directory
    // unmounts: the staging tmpfs lives in ns's mount namespace, but
    // the writeback walks the staging path, which is unaffected by the
    // unmounting of bind targets. We do however need the staging files
    // intact, so order is "walk first, then unmount".
    let directory_by_resource: std::collections::HashMap<&str, &MountedDirectory> =
        mounted_directories
            .iter()
            .map(|d| (d.resource_id.as_str(), d))
            .collect();
    let writeback_inputs: Vec<WritebackInput<'_>> = resource_channels
        .iter()
        .map(|channel| WritebackInput {
            channel,
            directory: directory_by_resource
                .get(channel.resource_id.as_str())
                .copied(),
            libsecret_state_path: if channel.libsecret {
                libsecret_state_path
            } else {
                None
            },
        })
        .collect();

    let writeback_result = writeback::dispatch_writeback(&writeback_inputs);

    // Always run mount cleanup, regardless of writeback success/failure.
    let cleanup_result = cleanup_mount_state(mounted_directories, has_files);

    match (writeback_result, cleanup_result) {
        (Ok(outcomes), Ok(())) => report_outcomes(&outcomes),
        (Err(e), _) => {
            eprintln!("underwrap-ns: end-of-run writeback: {e}");
        }
        (Ok(outcomes), Err(e)) => {
            report_outcomes(&outcomes);
            return Err(format!("mount cleanup failed: {e}"));
        }
    }

    if has_files {
        drop_caps().map_err(|e| format!("parent: dropping capabilities: {e}"))?;
    }

    propagate_exit_status(status)
}

fn cleanup_mount_state(
    mounted_directories: &[MountedDirectory],
    has_files: bool,
) -> Result<(), String> {
    cleanup_directory_mounts(mounted_directories).map_err(|e| format!("directory cleanup: {e}"))?;
    if has_files {
        cleanup_directory_staging_root(DIRECTORY_STAGING_ROOT)
            .map_err(|e| format!("directory staging cleanup: {e}"))?;
    }
    Ok(())
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

/// `UNDERWRAP_SUPPRESS_READONLY_WARNING=1` silences the stderr line
/// emitted when a writeback is dropped because the grant is read-only.
/// The structured journal event is always logged.
const SUPPRESS_READONLY_WARNING_ENV: &str = "UNDERWRAP_SUPPRESS_READONLY_WARNING";

fn read_only_warning_suppressed() -> bool {
    matches!(
        std::env::var(SUPPRESS_READONLY_WARNING_ENV).as_deref(),
        Ok("1")
    )
}

fn report_outcomes(outcomes: &[WritebackOutcome]) {
    let suppress = read_only_warning_suppressed();
    for outcome in outcomes {
        let resource_id = match outcome {
            WritebackOutcome::Unchanged | WritebackOutcome::Persisted => continue,
            WritebackOutcome::RejectedReadOnly { resource_id }
            | WritebackOutcome::DroppedReadOnly { resource_id } => resource_id,
        };
        eprintln!(
            r#"{{"audit":"writeback_rejected","reason":"grant_readonly","resource":"{resource_id}"}}"#,
        );
        if !suppress {
            eprintln!(
                "underwrap: changes to resource '{resource_id}' were not \
                 persisted (grant is read-only). Set \
                 {SUPPRESS_READONLY_WARNING_ENV}=1 to silence this message.",
            );
        }
    }
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

/// Read the execution bundle from the pipe on fd 3.
fn read_secret_fd() -> Result<ExecBundle, std::io::Error> {
    use std::os::unix::io::FromRawFd;

    // SAFETY: fd 3 is expected to be a valid pipe read-end passed by underwrap-exec.
    let mut file = unsafe { std::fs::File::from_raw_fd(SECRET_FD) };
    underwrap_protocol::ipc::read_frame_sync(&mut file).map_err(std::io::Error::other)
}
