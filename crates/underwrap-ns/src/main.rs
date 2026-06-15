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

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;

mod boundary;
mod caps;
mod directory_credential;
mod env;
mod idmap;
mod journal;
mod libsecret;
mod mounts;
mod resource_channel;
mod target_uid;
mod writeback;

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use caps::{drop_caps, set_no_new_privs};
use directory_credential::{DirectoryMountPlan, plan_directory_mounts, realize_directory_mounts};
use env::build_target_env;
use libsecret::setup_libsecret;
use mounts::{
    make_mount_namespace_private, remount_proc, setup_directory_staging_tmpfs,
    setup_mount_namespace,
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

    // Story 2600 / 2670: the root-owned table must assign this target a
    // dedicated uid. The uid is read directly from root state keyed on the
    // target path — never from the wire (no escalation / confused-deputy path).
    // Missing table/row is a hard error: never fall back to running as the
    // caller.
    let isolation = target_uid::resolve_isolation(&target)
        .map_err(|e| format!("resolving target isolation: {e}"))?;

    // The caller-owned directories to idmap caller<->X into the target's mount
    // namespace. `access_runtime_dir` appends the caller's runtime dir
    // (`<xdg_runtime_dir_base>/<caller-uid>`) — the same path env.rs points
    // XDG_RUNTIME_DIR at — resolved here from the caller's runtime uid so it is
    // correct for any authorized caller (the uid is never taken from the wire).
    let idmap_paths = build_idmap_paths(&exec_bundle, target_identity.uid);

    // Story 2640: build the caller<->X idmap user namespace BEFORE unsharing the
    // PID namespace — a helper fork after CLONE_NEWPID would become PID 1 of the
    // new namespace and break the real target fork. The idmapped mounts are
    // applied below, after the private mount namespace exists.
    let idmap_userns = if idmap_paths.is_empty() {
        None
    } else {
        let caller_gid = unsafe { libc::getgid() };
        Some(
            idmap::create_idmap_userns(target_identity.uid, caller_gid, isolation)
                .map_err(|e| format!("creating idmap user namespace: {e}"))?,
        )
    };

    make_process_non_dumpable()?;
    prepare_namespace()?;

    // Apply the idmapped bind mounts now: fsuid is still the caller (so open_tree
    // traverses the caller's home as owner), caps are held, and we are inside the
    // private mount namespace, so the forked child (which becomes X) inherits the
    // mounts and the host's view is untouched.
    if let Some(ref userns) = idmap_userns {
        idmap::mount_idmapped_dirs(&idmap_paths, &target_identity.home, userns)
            .map_err(|e| format!("setting up idmapped directories: {e}"))?;
    }

    let resource_channel = resource_channel::checkout(&exec_bundle.resource)?;

    // Compute the deterministic files-slot layout (pure — no filesystem side
    // effects). The parent keeps it to drive the end-of-run writeback walk; the
    // child realizes it after switching to its dedicated uid X, where fsuid is
    // naturally X and the staging is readable/writable by the target.
    let directory_layout = plan_files_mounts(&resource_channel)?;

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
            has_files,
            &directory_layout,
        )
    } else {
        run_parent(
            pid,
            &directory_layout,
            &resource_channel,
            has_files,
            isolation,
            libsecret_state_path.as_deref(),
        )
    }
}

/// The caller-owned directories to idmap caller<->X, drawn from the configured
/// `access_paths` plus — when `access_runtime_dir` is set — the caller's runtime
/// dir derived as `<xdg_runtime_dir_base>/<caller_uid>` (identical to the
/// `XDG_RUNTIME_DIR` env.rs sets). `caller_uid` is ns's own `getuid()`, never a
/// wire-supplied value.
fn build_idmap_paths(exec_bundle: &ExecBundle, caller_uid: libc::uid_t) -> Vec<String> {
    let mut paths = exec_bundle.access_paths.clone();
    if exec_bundle.access_runtime_dir {
        let runtime_dir = Path::new(&exec_bundle.xdg_runtime_dir_base).join(caller_uid.to_string());
        paths.push(runtime_dir.to_string_lossy().into_owned());
    }
    paths
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

fn prepare_namespace() -> Result<(), String> {
    // Always create a fresh PID + mount namespace. The mount namespace is
    // unconditional so every mode can remount /proc against the new PID
    // namespace (see remount_proc), limiting process discovery uniformly. The
    // mandatory uid-X boundary cuts both ways (story 2600/2645): a target
    // sharing the caller's IPC namespace could read the caller's SysV IPC /
    // POSIX mqueues, so add CLONE_NEWIPC (and CLONE_NEWUTS as cheap
    // defense-in-depth). These need CAP_SYS_ADMIN, which we still hold here.
    let flags = libc::CLONE_NEWPID | libc::CLONE_NEWNS | libc::CLONE_NEWIPC | libc::CLONE_NEWUTS;
    let ret = unsafe { libc::unshare(flags) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("unshare failed: {err}"));
    }

    make_mount_namespace_private()
        .map_err(|e| format!("making mount namespace private failed: {e}"))?;

    Ok(())
}

/// Compute the files-slot mount layout (pure). Returns an empty `Vec` when the
/// wrapper does not project files.
fn plan_files_mounts(
    resource_channel: &ResourceChannel,
) -> Result<Vec<DirectoryMountPlan>, String> {
    if resource_channel.files_target_paths.is_empty() {
        return Ok(Vec::new());
    }
    plan_directory_mounts(
        &resource_channel.files_target_paths,
        &PathBuf::from(DIRECTORY_STAGING_ROOT),
    )
    .map_err(|e| format!("planning files-slot mounts: {e}"))
}

/// Decode the Checkout blob and realize the planned files-slot bind-mounts
/// (create + materialize + bind). Runs in the child after switching to the
/// dedicated target uid, so fsuid is naturally X. No-ops when the wrapper does
/// not project files.
fn realize_files_mounts(
    resource_channel: &ResourceChannel,
    target_home: &Path,
    plans: &[DirectoryMountPlan],
) -> Result<(), String> {
    if plans.is_empty() {
        return Ok(());
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
    realize_directory_mounts(
        resource_channel.resource_id.as_str(),
        &blob,
        target_home,
        plans,
    )
    .map_err(|e| format!("realizing files-slot mounts: {e}"))
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
    isolation: target_uid::Isolation,
    has_files: bool,
    directory_layout: &[DirectoryMountPlan],
) -> Result<(), String> {
    // Inside the new PID namespace: mount a fresh /proc in every mode so the
    // target only sees namespace-local PIDs.
    remount_proc().map_err(|e| format!("remounting /proc failed: {e}"))?;

    // Story 2600: switch the target (and thus its whole subtree) to its dedicated
    // uid X BEFORE dropping caps — the switch needs CAP_SETUID/CAP_SETGID, which
    // are retained across it (no uid 0 on either side) and cleared by drop_caps
    // below. A failed switch aborts the run (switch_to bails): never exec a target
    // that did not drop to X. The switch must precede every X-facing mount step
    // below so they run at the natural fsuid X.
    target_uid::switch_to(isolation)
        .map_err(|e| format!("switching to dedicated target uid: {e}"))?;

    // Files projection: realize the staging here, at fsuid X, so the tmpfs root,
    // its materialized files, and the mountpoints are X-owned and the uid-X
    // target can read/write them; the bind lands over any idmapped home set up
    // pre-fork.
    if has_files {
        setup_directory_staging_tmpfs(DIRECTORY_STAGING_ROOT)
            .map_err(|e| format!("directory staging setup (isolated): {e}"))?;
        realize_files_mounts(resource_channel, &target_identity.home, directory_layout)?;
    }

    // Private libsecret bus: this runs after the switch, so the bus tmpfs is
    // X-owned and the dbus-daemon / secretsd the child forks (also X) can create
    // the socket and write the state file.
    if libsecret_enabled {
        setup_mount_namespace().map_err(|e| format!("mount namespace setup failed: {e}"))?;
    }

    set_no_new_privs().map_err(|e| format!("setting no_new_privs: {e}"))?;

    // Boundary hardening (2645): the uid-X target must not reach the caller's
    // session keyring.
    boundary::join_session_keyring().map_err(|e| format!("joining session keyring: {e}"))?;

    drop_caps().map_err(|e| format!("dropping capabilities: {e}"))?;

    // Verify the drop is real before exec (Setuid Demystified): ids fully
    // switched, caps gone, regain impossible. Bail otherwise.
    target_uid::verify_switched().map_err(|e| format!("verifying target uid switch: {e}"))?;

    disable_core_dumps()?;
    // Belt-and-suspenders: the upcoming exec resets dumpable to SUID_DUMP_USER
    // for an isolated (real==eff==saved==X) target — the uid gate (caller != X),
    // not dumpable, is the real protection. This only covers the brief pre-exec
    // window.
    make_process_non_dumpable()?;

    // Sever the controlling tty (TIOCSTI guard) just before building argv/env.
    boundary::sever_controlling_tty().map_err(|e| format!("severing controlling tty: {e}"))?;

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

    // Last fd touch before exec: sweep every inherited fd >= 3 to CLOEXEC so no
    // privileged fd leaks across the boundary into the uid-X target.
    boundary::close_inherited_fds_for_exec()
        .map_err(|e| format!("closing inherited fds before exec: {e}"))?;

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
    directory_layout: &[DirectoryMountPlan],
    resource_channel: &ResourceChannel,
    has_files: bool,
    isolation: target_uid::Isolation,
    libsecret_state_path: Option<&Path>,
) -> Result<(), String> {
    // Keep CAP_SETUID/CAP_SETGID through writeback only when the supervisor must
    // read X-owned files/libsecret state. Env-only targets have no X-owned
    // checkout surface to read, so drop caps before waiting.
    let needs_fsuid_window = has_files || resource_channel.libsecret;
    if !needs_fsuid_window {
        drop_caps().map_err(|e| format!("parent: dropping capabilities: {e}"))?;
    }

    let status = wait_for_child(pid)?;

    // Compose and dispatch the end-of-run write while the staging tmpfs still
    // lives in this namespace. It will be torn down with the namespace when the
    // supervisor exits.
    let writeback_input = WritebackInput {
        channel: resource_channel,
        directory_layout,
        libsecret_state_path: if resource_channel.libsecret {
            libsecret_state_path
        } else {
            None
        },
    };

    // X-owned staging / libsecret state is read inside the single setfsuid(X)
    // window (which also bounds the symlink-TOCTOU walk to uid X). Env-only
    // targets have nothing X-owned to read and skip it.
    let writeback_result = if needs_fsuid_window {
        target_uid::with_target_fsuid(isolation, || {
            writeback::dispatch_writeback(&[writeback_input])
        })
        .and_then(|inner| inner)
    } else {
        writeback::dispatch_writeback(&[writeback_input])
    };

    match writeback_result {
        Ok(outcomes) => report_outcomes(&outcomes),
        Err(e) => {
            eprintln!("underwrap-ns: end-of-run writeback: {e}");
        }
    }

    if needs_fsuid_window {
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
