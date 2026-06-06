//! underwrap-ns — PID namespace isolation stage.
//!
//! Reads the `ExecBundle` on fd 3, opens the wrapper-resource channel
//! handed to it by `underwrap-exec` (fd 20), drives `Checkout`,
//! materializes any `files` slot into a tmpfs staging dir, starts a
//! private dbus-daemon + secretsd in the namespace when the resource
//! has the libsecret slot enabled, then forks into a fresh PID
//! namespace to exec the target.
//!
//! After waitpid, walks the staging tmpfs, reads the libsecret state
//! file secretsd wrote, composes a new `ResourceBlob` and dispatches
//! the end-of-run decision (`UsageSignal` / `Checkin` / drop+warn) on
//! the channel.

mod boundary;
mod caps;
mod directory_credential;
mod env;
mod journal;
mod libsecret;
mod mounts;
mod resource_channel;
mod target_uid;
mod writeback;

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use caps::{drop_caps, set_no_new_privs};
use directory_credential::{DirectoryMount, cleanup_directory_mounts, prepare_directory_mounts};
use env::build_target_env;
use libsecret::setup_libsecret;
use mounts::{
    cleanup_directory_staging_root, make_mount_namespace_private, remount_proc,
    setup_directory_staging_tmpfs, setup_mount_namespace,
};
use resource_channel::ResourceChannel;
use underwrap_protocol::exec::ExecBundle;
use writeback::{WritebackInput, WritebackOutcome};

/// The fd number used to pass the exec bundle from underwrap-exec.
const SECRET_FD: i32 = 3;

pub(crate) const DIRECTORY_STAGING_ROOT: &str = "/tmp/.underwrap-ns-directories";
const LIBSECRET_BUS_DIR: &str = "/tmp/.underwrap-ns-bus";
const LIBSECRET_STATE_FILENAME: &str = "libsecret-state.jcs";

struct TargetIdentity {
    uid: libc::uid_t,
    home: PathBuf,
}

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
    let libsecret_resource_present = exec_bundle.resource.libsecret;
    let libsecret_enabled = libsecret_requested && libsecret_resource_present;
    let has_files = !exec_bundle.resource.files_target_paths.is_empty();
    let target_identity =
        resolve_target_identity().map_err(|e| format!("resolving target identity: {e}"))?;

    // Story 2600: does the root-owned table assign this target a dedicated uid?
    // The uid is read directly from root state keyed on the target path — never
    // from the wire (no escalation / confused-deputy path).
    let isolation = target_uid::resolve_isolation(&target)
        .map_err(|e| format!("resolving target isolation: {e}"))?;
    // Files-slot / libsecret isolation needs the per-uid staging path (the
    // child materializes/binds as X and the supervisor reads back under a single
    // setfsuid window). That lands with the 2630 files-handling / 2640 work; until
    // then, refuse rather than fall back to an unisolated target (which would put
    // the secret back in a same-uid-readable env). Fail closed.
    if isolation.is_some() && (has_files || libsecret_enabled) {
        return Err(
            "isolate is not yet supported together with files or libsecret slots; \
             refusing to run rather than fall back to an unisolated target"
                .to_string(),
        );
    }

    make_process_non_dumpable()?;
    prepare_namespace(has_files, isolation.is_some())?;

    let resource_channel = resource_channel::checkout(&exec_bundle.resource)?;

    let directory_mounts = prepare_files_mounts(&resource_channel, &target_identity.home)?;

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
            &target_identity,
            &resource_channel,
            libsecret_enabled,
            libsecret_state_path.as_deref(),
            isolation,
        )
    } else {
        run_parent(
            pid,
            &directory_mounts,
            &resource_channel,
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

fn prepare_namespace(has_files: bool, isolated: bool) -> Result<(), String> {
    // Always create a fresh PID + mount namespace. The mount namespace is
    // unconditional now so that every mode can remount /proc against the new
    // PID namespace (see remount_proc), limiting process discovery uniformly.
    // For an isolated target the boundary cuts both ways (story 2600/2645): a
    // uid-X target sharing the caller's IPC namespace could read the caller's
    // SysV IPC / POSIX mqueues, so add CLONE_NEWIPC (and CLONE_NEWUTS as cheap
    // defense-in-depth). These need CAP_SYS_ADMIN, which we still hold here.
    let mut flags = libc::CLONE_NEWPID | libc::CLONE_NEWNS;
    if isolated {
        flags |= libc::CLONE_NEWIPC | libc::CLONE_NEWUTS;
    }
    let ret = unsafe { libc::unshare(flags) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("unshare failed: {err}"));
    }

    make_mount_namespace_private()
        .map_err(|e| format!("making mount namespace private failed: {e}"))?;

    if has_files {
        setup_directory_staging_tmpfs(DIRECTORY_STAGING_ROOT)
            .map_err(|e| format!("directory staging setup failed: {e}"))?;
    }

    Ok(())
}

/// Decode the Checkout blob and set up the resource's files-slot
/// bind-mounts inside the namespace. Returns an empty `Vec` when the
/// wrapper does not project files — empty `Vec` means "no files-slot
/// staging to walk at writeback time."
fn prepare_files_mounts(
    resource_channel: &ResourceChannel,
    target_home: &Path,
) -> Result<Vec<DirectoryMount>, String> {
    if resource_channel.files_target_paths.is_empty() {
        return Ok(Vec::new());
    }
    let blob = underwrap_protocol::resource_blob::ResourceBlob::from_canonical_bytes(
        &resource_channel.original_canonical,
    )
    .map_err(|e| {
        format!(
            "decoding Checkout blob for '{}': {e}",
            resource_channel.resource_id
        )
    })?;
    prepare_directory_mounts(
        resource_channel.resource_id.as_str(),
        &blob,
        &resource_channel.files_target_paths,
        target_home,
        &PathBuf::from(DIRECTORY_STAGING_ROOT),
    )
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

#[allow(clippy::too_many_arguments)]
fn run_child(
    target: String,
    target_args: Vec<String>,
    exec_bundle: &ExecBundle,
    target_identity: &TargetIdentity,
    resource_channel: &ResourceChannel,
    libsecret_enabled: bool,
    libsecret_state_path: Option<&Path>,
    isolation: Option<target_uid::Isolation>,
) -> Result<(), String> {
    // Inside the new PID namespace: mount a fresh /proc in every mode so the
    // target only sees namespace-local PIDs.
    remount_proc().map_err(|e| format!("remounting /proc failed: {e}"))?;

    if libsecret_enabled {
        setup_mount_namespace().map_err(|e| format!("mount namespace setup failed: {e}"))?;
    }

    // Story 2600: switch the target (and thus its whole subtree) to its dedicated
    // uid X BEFORE dropping caps — the switch needs CAP_SETUID/CAP_SETGID, which
    // are retained across it (no uid 0 on either side) and cleared by drop_caps
    // below. A failed switch aborts the run (switch_to bails): never exec a target
    // that did not drop to X.
    if let Some(iso) = isolation {
        target_uid::switch_to(iso)
            .map_err(|e| format!("switching to dedicated target uid: {e}"))?;
    }

    set_no_new_privs().map_err(|e| format!("setting no_new_privs: {e}"))?;

    if isolation.is_some() {
        // Boundary hardening (2645): the uid-X target must not reach the caller's
        // session keyring.
        boundary::join_session_keyring().map_err(|e| format!("joining session keyring: {e}"))?;
    }

    drop_caps().map_err(|e| format!("dropping capabilities: {e}"))?;

    if isolation.is_some() {
        // Verify the drop is real before exec (Setuid Demystified): ids fully
        // switched, caps gone, regain impossible. Bail otherwise.
        target_uid::verify_switched().map_err(|e| format!("verifying target uid switch: {e}"))?;
    }

    disable_core_dumps()?;
    // Belt-and-suspenders: the upcoming exec resets dumpable to SUID_DUMP_USER
    // for an isolated (real==eff==saved==X) target — the uid gate (caller != X),
    // not dumpable, is the real protection. This only covers the brief pre-exec
    // window.
    make_process_non_dumpable()?;

    if isolation.is_some() {
        // Sever the controlling tty (TIOCSTI guard) just before building argv/env.
        boundary::sever_controlling_tty().map_err(|e| format!("severing controlling tty: {e}"))?;
    }

    let mut cmd = std::process::Command::new(&target);
    cmd.args(&target_args);
    cmd.env_clear();

    let host_env: std::collections::HashMap<String, String> = std::env::vars().collect();
    let target_env = build_target_env(
        &host_env,
        exec_bundle,
        &target_identity.home,
        target_identity.uid,
    )?;

    if libsecret_enabled {
        let state_path = libsecret_state_path
            .ok_or_else(|| "libsecret enabled but no state path provided".to_string())?;
        let section_bytes = libsecret_section_bytes(resource_channel)?;
        // The bus daemon and secretsd run with a sanitised environment drawn
        // from the trusted target env only, so caller-controlled
        // service-discovery paths cannot reach the private bus.
        let helper_env: std::collections::HashMap<String, String> =
            ["HOME", "PATH", "LANG", "LC_ALL", "TZ"]
                .iter()
                .filter_map(|k| target_env.get(*k).map(|v| ((*k).to_string(), v.clone())))
                .collect();
        setup_libsecret(
            &mut cmd,
            SECRET_FD,
            resource_channel.channel_fd,
            &helper_env,
            &section_bytes,
            state_path,
        )?;
    }

    for (key, value) in target_env {
        cmd.env(key, value);
    }

    resource_channel::close_channel_fd_for_exec(resource_channel);

    if isolation.is_some() {
        // Last fd touch before exec: sweep every inherited fd >= 3 to CLOEXEC so
        // no privileged fd leaks across the boundary into the uid-X target.
        boundary::close_inherited_fds_for_exec()
            .map_err(|e| format!("closing inherited fds before exec: {e}"))?;
    }

    let err = cmd.exec();
    Err(format!("exec failed: {err}"))
}

fn libsecret_section_bytes(channel: &ResourceChannel) -> Result<Vec<u8>, String> {
    if !channel.libsecret {
        return Err("libsecret enabled but resource does not project libsecret".to_string());
    }
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
    directory_mounts: &[DirectoryMount],
    resource_channel: &ResourceChannel,
    has_files: bool,
    libsecret_state_path: Option<&Path>,
) -> Result<(), String> {
    if !has_files {
        drop_caps().map_err(|e| format!("parent: dropping capabilities: {e}"))?;
    }

    let status = wait_for_child(pid)?;

    // Compose and dispatch the end-of-run write BEFORE the directory
    // unmounts: the staging tmpfs lives in ns's mount namespace, but
    // the writeback walks the staging path, which is unaffected by the
    // unmounting of bind targets. We do however need the staging files
    // intact, so order is "walk first, then unmount".
    let writeback_input = WritebackInput {
        channel: resource_channel,
        directory_mounts,
        libsecret_state_path: if resource_channel.libsecret {
            libsecret_state_path
        } else {
            None
        },
    };

    let writeback_result = writeback::dispatch_writeback(&[writeback_input]);

    // Always run mount cleanup, regardless of writeback success/failure.
    let cleanup_result = cleanup_mount_state(directory_mounts, has_files);

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

fn cleanup_mount_state(directory_mounts: &[DirectoryMount], has_files: bool) -> Result<(), String> {
    cleanup_directory_mounts(directory_mounts).map_err(|e| format!("directory cleanup: {e}"))?;
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
/// The structured audit event is always emitted — on systemd hosts it
/// goes to journald's native socket, and only falls back to stderr
/// when the journal socket is unreachable (so the audit trail still
/// exists on non-systemd dev runs and in tests).
const SUPPRESS_READONLY_WARNING_ENV: &str = "UNDERWRAP_SUPPRESS_READONLY_WARNING";

/// `PRIORITY=4` — `LOG_WARNING` in syslog terms; matches the severity
/// of the human-readable warning that accompanies the event.
const JOURNAL_PRIORITY_WARNING: &str = "4";
const JOURNAL_SYSLOG_IDENTIFIER: &str = "underwrap-ns";

fn read_only_warning_suppressed() -> bool {
    matches!(
        std::env::var(SUPPRESS_READONLY_WARNING_ENV).as_deref(),
        Ok("1")
    )
}

fn report_outcomes(outcomes: &[WritebackOutcome]) {
    let suppress = read_only_warning_suppressed();
    for outcome in outcomes {
        let (resource_id, resource_name, reason) = match outcome {
            WritebackOutcome::Unchanged | WritebackOutcome::Persisted => continue,
            // Wrapper-local decision: the Checkout's writability bit
            // was already false, so no Checkin was attempted.
            WritebackOutcome::DroppedReadOnly {
                resource_id,
                resource_name,
            } => (resource_id, resource_name, "grant_readonly"),
            // Server-side rejection: Checkin was sent but the daemon
            // refused it (e.g. the grant flipped mid-run).
            WritebackOutcome::RejectedReadOnly {
                resource_id,
                resource_name,
            } => (resource_id, resource_name, "server_rejected"),
        };

        emit_audit_event(reason, resource_id, resource_name);

        if !suppress {
            eprintln!(
                "underwrap: changes to resource '{resource_name}' ({resource_id}) \
                 were not persisted (grant is read-only). Set \
                 {SUPPRESS_READONLY_WARNING_ENV}=1 to silence this message.",
            );
        }
    }
}

/// Send the `writeback_rejected` event to journald. On non-systemd
/// hosts (or if the journal socket send fails), fall back to a
/// stderr-JSON line so the audit trail is not lost on dev / test runs.
fn emit_audit_event(reason: &str, resource_id: &str, resource_name: &str) {
    let message =
        format!("writeback rejected for resource '{resource_name}' ({resource_id}): {reason}");
    let fields = [
        ("MESSAGE", message.as_str()),
        ("PRIORITY", JOURNAL_PRIORITY_WARNING),
        ("SYSLOG_IDENTIFIER", JOURNAL_SYSLOG_IDENTIFIER),
        ("AUDIT", "writeback_rejected"),
        ("REASON", reason),
        ("RESOURCE_ID", resource_id),
        ("RESOURCE_NAME", resource_name),
    ];
    if journal::send_record(&fields).is_err() {
        // Fallback: emit the same record as one JSON line on stderr.
        // The journal field names map 1:1 to the JSON keys so log
        // pipelines can be written once.
        eprintln!(
            r#"{{"audit":"writeback_rejected","reason":"{reason}","resource_id":"{resource_id}","resource_name":"{resource_name}"}}"#,
        );
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

fn resolve_target_identity() -> Result<TargetIdentity, std::io::Error> {
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
    Ok(TargetIdentity {
        uid,
        home: PathBuf::from(home),
    })
}

/// Read the execution bundle from the pipe on fd 3.
fn read_secret_fd() -> Result<ExecBundle, std::io::Error> {
    use std::os::unix::io::FromRawFd;

    // SAFETY: fd 3 is expected to be a valid pipe read-end passed by underwrap-exec.
    let mut file = unsafe { std::fs::File::from_raw_fd(SECRET_FD) };
    underwrap_protocol::ipc::read_frame_sync(&mut file).map_err(std::io::Error::other)
}
