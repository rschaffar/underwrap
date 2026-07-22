//! underwrap-ns — PID namespace isolation stage.
//!
//! First authenticates the fixed fd-20 wrapper-resource channel as a socketpair
//! created by the selected instance daemon. After the secret-free idmap helper
//! is reaped, it creates the private exec gate, reads the `ExecBundle` on fd 3,
//! drives `Checkout`, and completely plans the files projection. The supervisor
//! then unshares PID, mount, IPC, and UTS namespaces, makes mount propagation
//! private, realizes idmapped caller-directory mounts, and forks.
//!
//! The child enters the new PID namespace, remounts `/proc`, switches to the
//! dedicated target identity, realizes the planned files projection and any
//! private libsecret service, and reports its final pre-exec gate state. The
//! parent observes the gate through EOF and then reaps the child; both an
//! accepted gate state and successful reap are required before it collects
//! staging, composes the post-run `ResourceBlob`, and dispatches the end-of-run
//! decision (`UsageSignal` / `Checkin` / drop+warn). This records an authorized
//! lifecycle, not proof of exec: normal targets may finalize their changes, but
//! the narrow false-positive pre-exec-death ambiguity can finalize at most the
//! fully materialized unchanged state.

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;

mod boundary;
mod caps;
mod directory_credential;
mod env;
mod exec_gate;
mod handoff;
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
use directory_credential::{
    DirectoryMountPlan, DirectoryProjectionPlan, plan_directory_projection,
    realize_directory_mounts,
};
use env::build_target_env;
use exec_gate::{ExecGate, GateAuthorization, GateReader, GateWriter};
use libsecret::setup_libsecret;
use mounts::{
    make_mount_namespace_private, remount_proc, setup_directory_staging_tmpfs,
    setup_mount_namespace,
};
use resource_channel::ResourceChannel;
use underwrap_protocol::exec::{EXEC_BUNDLE_FD, ExecBundle, IDMAP_REQUIRED_ENV};
use underwrap_protocol::resource_blob::ResourceBlob;
use writeback::{WritebackInput, WritebackOutcome};

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
    // Executing a file-capability wrapper resets dumpability. Close that window
    // before reading selectors, argv, or secret-bearing handoff bytes.
    make_process_non_dumpable()?;

    // fd 20 is the bearer capability for this invocation. `underwrap-ns` is
    // world-executable, so authenticate the socketpair's immutable creation-time
    // peer credentials before trusting fd 3, the wrapper selector, target argv,
    // or any privileged namespace input.
    let iso_instance = require_isolation_selector("UNDERWRAP_ISOLATION_INSTANCE")?;
    let daemon_identity = handoff::authenticate_instance_channel(&iso_instance)
        .map_err(|e| format!("authenticating exec handoff: {e}"))?;

    // The root-owned table must assign this `(instance, wrapper)` pair a
    // dedicated UID. Only the instance has been used before channel
    // authentication, and solely to resolve the trusted daemon account.
    let iso_wrapper = require_isolation_selector("UNDERWRAP_ISOLATION_WRAPPER")?;
    let isolation = target_uid::resolve_isolation(&iso_instance, &iso_wrapper)
        .map_err(|e| format!("resolving target isolation: {e}"))?;
    handoff::require_distinct_target(&daemon_identity, isolation.uid, isolation.gid)?;

    let target_identity =
        resolve_target_identity().map_err(|e| format!("resolving target identity: {e}"))?;
    if target_identity.uid == isolation.uid {
        return Err(format!(
            "caller uid {} overlaps selected target uid {}; refusing handoff",
            target_identity.uid, isolation.uid
        ));
    }

    // Idmap creation forks a helper that must briefly be dumpable so this
    // non-dumpable parent can write its uid/gid maps. The authenticated exec
    // stage supplies a boolean hint, allowing that helper to be created before
    // either process reads fd 3 or Checkout copies secret bytes. The decoded
    // bundle is checked against the hint immediately afterward.
    let idmap_required = idmap_required_hint()?;
    let idmap_userns = if idmap_required {
        let caller_gid = unsafe { libc::getgid() };
        Some(
            idmap::create_idmap_userns(target_identity.uid, caller_gid, isolation)
                .map_err(|e| format!("creating idmap user namespace: {e}"))?,
        )
    } else {
        None
    };

    // Create the private gate after the secret-free idmap helper is reaped and
    // while fixed fd 3 and authenticated fd 20 are both still occupied. It is
    // local to this privileged stage and carries no selectors or secret bytes.
    let exec_gate = ExecGate::new()?;

    let (target, target_args) = parse_target_invocation(std::env::args().collect())?;
    let exec_bundle = read_secret_fd()
        .map_err(|e| format!("reading exec bundle from fd {EXEC_BUNDLE_FD}: {e}"))?;
    let has_access_paths = !exec_bundle.access_paths.is_empty();
    if idmap_required != has_access_paths {
        return Err(
            "authenticated idmap hint does not match ExecBundle access_paths; refusing handoff"
                .to_string(),
        );
    }

    // Checkout authenticates the resource protocol before namespace changes,
    // mounts, or a target-identity switch.
    let resource_channel = resource_channel::checkout(&exec_bundle.resource)?;

    let libsecret_requested = std::env::var("UNDERWRAP_LIBSECRET").is_ok();
    let libsecret_resource_present = exec_bundle.resource.libsecret;
    let libsecret_enabled = libsecret_requested && libsecret_resource_present;
    let has_files = !exec_bundle.resource.files_target_paths.is_empty();

    // Runtime-dir policy. An isolated target gets its OWN dedicated-uid runtime
    // dir (`<base>/<target-user-name>`, provisioned by the system) — a runtime
    // dir is a per-uid resource, so the target running as X gets X's. X's uid
    // comes from the root-owned table (never the wire).
    let xdg_runtime_dir = target_runtime_dir(&exec_bundle.target_runtime_dir_base, isolation.uid)?;

    // Decode Checkout once and complete every files routing/shape decision
    // before namespace, idmapped-mount, fork, staging, uid-switch, or helper
    // effects. The child can therefore realize only a completed semantic plan.
    let resource_blob = ResourceBlob::from_canonical_bytes(&resource_channel.original_canonical)
        .map_err(|e| {
            format!(
                "decoding Checkout blob for '{}': {e}",
                resource_channel.resource_id
            )
        })?;
    let directory_plan = plan_files_projection(&resource_channel, resource_blob)?;

    let libsecret_state_path = if libsecret_enabled {
        Some(PathBuf::from(format!(
            "{LIBSECRET_BUS_DIR}/{LIBSECRET_STATE_FILENAME}"
        )))
    } else {
        None
    };

    // The caller-owned directories to idmap caller<->X into the target's mount
    // namespace (`~`/`~/rel`/absolute), straight from authenticated wrapper
    // configuration in the exec bundle.
    let idmap_paths = exec_bundle.access_paths.clone();

    prepare_namespace()?;

    // Apply the idmapped bind mounts now: fsuid is still the caller (so open_tree
    // traverses the caller's home as owner), caps are held, and we are inside the
    // private mount namespace, so the forked child (which becomes X) inherits the
    // mounts and the host's view is untouched.
    if let Some(ref userns) = idmap_userns {
        idmap::mount_idmapped_dirs(&idmap_paths, &target_identity.home, userns)
            .map_err(|e| format!("setting up idmapped directories: {e}"))?;
    }

    let pid = fork_into_namespace()?;
    if pid == 0 {
        run_namespace_child(
            exec_gate.into_child(),
            target,
            target_args,
            &exec_bundle,
            &target_identity,
            &xdg_runtime_dir,
            &resource_channel,
            libsecret_enabled,
            libsecret_state_path.as_deref(),
            isolation,
            has_files,
            &directory_plan,
        )
    } else {
        run_parent(
            pid,
            exec_gate.into_parent(),
            directory_plan.mounts(),
            &resource_channel,
            has_files,
            isolation,
            libsecret_state_path.as_deref(),
        )
    }
}

/// The executing dedicated uid X's own provisioned runtime dir,
/// `<base>/<target-user-name>` — matching exactly what the NixOS module
/// provisions (keyed on the target user *name*, resolved here from X via
/// `getpwuid`). This is the default `XDG_RUNTIME_DIR`: a runtime dir is a
/// per-uid resource, so the target running as X gets X's own.
fn target_runtime_dir(base: &str, target_uid: libc::uid_t) -> Result<String, String> {
    if !Path::new(base).is_absolute() {
        return Err(format!(
            "target runtime dir base must be absolute, got {base:?}"
        ));
    }
    let name = target_uid::user_name(target_uid)?;
    Path::new(base)
        .join(&name)
        .to_str()
        .map(str::to_string)
        .ok_or_else(|| format!("target runtime dir under {base:?} is not valid UTF-8"))
}

/// Read a mandatory isolation selector env var (`UNDERWRAP_ISOLATION_INSTANCE`
/// / `_WRAPPER`) set by underwrap-exec. Missing or empty is a hard error — under
/// mandatory isolation we never fall back to running the target as the caller.
fn require_isolation_selector(var: &str) -> Result<String, String> {
    match std::env::var(var) {
        Ok(v) if !v.is_empty() => Ok(v),
        Ok(_) => Err(format!("isolation selector {var} is empty")),
        Err(_) => Err(format!(
            "isolation selector {var} is not set (underwrap-exec must provide it); \
             refusing to run the target as the caller"
        )),
    }
}

fn idmap_required_hint() -> Result<bool, String> {
    match std::env::var(IDMAP_REQUIRED_ENV) {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Ok(value) if value == "1" => Ok(true),
        Ok(value) => Err(format!(
            "invalid authenticated idmap hint {IDMAP_REQUIRED_ENV}={value:?}"
        )),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!(
            "authenticated idmap hint {IDMAP_REQUIRED_ENV} is not valid UTF-8"
        )),
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

fn prepare_namespace() -> Result<(), String> {
    // Always create a fresh PID + mount namespace. The mount namespace is
    // unconditional so every mode can remount /proc against the new PID
    // namespace (see remount_proc), limiting process discovery uniformly. The
    // A target sharing the caller's IPC namespace could reach caller SysV IPC
    // and POSIX message queues, so create IPC and UTS namespaces as well. This
    // setup still holds CAP_SYS_ADMIN.
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

/// Purely validate and route the complete files projection. Returns an empty
/// plan when this wrapper does not project files; in that mode the blob's files
/// section remains operator-owned and is preserved verbatim at finalization.
fn plan_files_projection(
    resource_channel: &ResourceChannel,
    blob: ResourceBlob,
) -> Result<DirectoryProjectionPlan, String> {
    plan_directory_projection(
        &resource_channel.files_target_paths,
        &PathBuf::from(DIRECTORY_STAGING_ROOT),
        &resource_channel.resource_id,
        blob,
    )
    .map_err(|e| format!("planning complete files projection: {e}"))
}

/// Realize only a completed files projection after switching to the dedicated
/// target uid, so fsuid is naturally X.
fn realize_files_mounts(target_home: &Path, plan: &DirectoryProjectionPlan) -> Result<(), String> {
    if plan.is_empty() {
        return Ok(());
    }
    realize_directory_mounts(target_home, plan)
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
fn run_namespace_child(
    mut gate_writer: GateWriter,
    target: String,
    target_args: Vec<String>,
    exec_bundle: &ExecBundle,
    target_identity: &TargetIdentity,
    xdg_runtime_dir: &str,
    resource_channel: &ResourceChannel,
    libsecret_enabled: bool,
    libsecret_state_path: Option<&Path>,
    isolation: target_uid::Isolation,
    has_files: bool,
    directory_plan: &DirectoryProjectionPlan,
) -> ! {
    let error = run_child(
        target,
        target_args,
        exec_bundle,
        target_identity,
        xdg_runtime_dir,
        resource_channel,
        libsecret_enabled,
        libsecret_state_path,
        isolation,
        has_files,
        directory_plan,
        &mut gate_writer,
    )
    .expect_err("target exec does not return on success");

    // Every controlled child-side return reports failure. If this write itself
    // fails, EOF or a truncated stream still suppresses parent dispatch.
    if let Err(gate_error) = gate_writer.send_failed() {
        eprintln!("underwrap-ns: {gate_error}");
    }
    eprintln!("underwrap-ns: {error}");
    // SAFETY: this is the post-fork child error path. `_exit` avoids running
    // duplicated parent-side destructors; the kernel closes every owned fd.
    unsafe { libc::_exit(1) }
}

#[allow(clippy::too_many_arguments)]
fn run_child(
    target: String,
    target_args: Vec<String>,
    exec_bundle: &ExecBundle,
    target_identity: &TargetIdentity,
    xdg_runtime_dir: &str,
    resource_channel: &ResourceChannel,
    libsecret_enabled: bool,
    libsecret_state_path: Option<&Path>,
    isolation: target_uid::Isolation,
    has_files: bool,
    directory_plan: &DirectoryProjectionPlan,
    gate_writer: &mut GateWriter,
) -> Result<(), String> {
    // Inside the new PID namespace: mount a fresh /proc in every mode so the
    // target only sees namespace-local PIDs.
    remount_proc().map_err(|e| format!("remounting /proc failed: {e}"))?;

    // Switch the target subtree to its dedicated UID before dropping
    // CAP_SETUID/CAP_SETGID. The switch precedes target-facing mount setup so
    // those files are created under the target's natural fsuid. Any failure
    // aborts the run.
    target_uid::switch_to(isolation)
        .map_err(|e| format!("switching to dedicated target uid: {e}"))?;

    // Files projection: realize the staging here, at fsuid X, so the tmpfs root,
    // its materialized files, and the mountpoints are X-owned and the uid-X
    // target can read/write them; the bind lands over any idmapped home set up
    // pre-fork.
    if has_files {
        setup_directory_staging_tmpfs(DIRECTORY_STAGING_ROOT)
            .map_err(|e| format!("directory staging setup (isolated): {e}"))?;
        realize_files_mounts(&target_identity.home, directory_plan)?;
    }

    // Private libsecret bus: this runs after the switch, so the bus tmpfs is
    // X-owned and the dbus-daemon / secretsd the child forks (also X) can create
    // the socket and write the state file.
    if libsecret_enabled {
        setup_mount_namespace().map_err(|e| format!("mount namespace setup failed: {e}"))?;
    }

    set_no_new_privs().map_err(|e| format!("setting no_new_privs: {e}"))?;

    // Give the target a fresh session keyring rather than the caller's.
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
        xdg_runtime_dir,
    )?;

    if libsecret_enabled {
        let state_path = libsecret_state_path
            .ok_or_else(|| "libsecret enabled but no state path provided".to_string())?;
        let section_bytes = libsecret_section_bytes(resource_channel, directory_plan.blob())?;
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
            EXEC_BUNDLE_FD,
            resource_channel.channel_fd,
            gate_writer.raw_fd(),
            &helper_env,
            &section_bytes,
            state_path,
        )?;
    }

    for (key, value) in target_env {
        cmd.env(key, value);
    }

    resource_channel::close_channel_fd(resource_channel)
        .map_err(|e| format!("closing resource channel before exec: {e}"))?;

    // Last fd protection before exec: sweep every inherited fd >= 3 to CLOEXEC
    // so no privileged fd leaks into the uid-X target. The gate was CLOEXEC at
    // creation; Armed is written only after this final controlled operation.
    boundary::close_inherited_fds_for_exec()
        .map_err(|e| format!("protecting inherited fds before exec: {e}"))?;
    gate_writer.send_armed()?;

    let err = cmd.exec();
    Err(format!("exec failed: {err}"))
}

fn libsecret_section_bytes(
    channel: &ResourceChannel,
    blob: &ResourceBlob,
) -> Result<Vec<u8>, String> {
    if !channel.libsecret {
        return Err("libsecret enabled but resource does not project libsecret".to_string());
    }
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
    gate_reader: GateReader,
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

    // Read through EOF before reaping. Successful exec closes the CLOEXEC
    // writer; controlled returns append Failed. The later wait result is also
    // required before this authorization can drive dispatch.
    let authorization = gate_reader.observe();
    let status = match wait_for_child(pid) {
        Ok(status) => status,
        Err(wait_error) => {
            let close_error = resource_channel::close_channel_fd(resource_channel).err();
            let drop_error = if needs_fsuid_window {
                drop_caps()
                    .err()
                    .map(|e| format!("parent: dropping capabilities: {e}"))
            } else {
                None
            };
            return Err(format!(
                "{wait_error}{}{}",
                close_error
                    .map(|e| format!("; closing resource channel: {e}"))
                    .unwrap_or_default(),
                drop_error.map(|e| format!("; {e}")).unwrap_or_default()
            ));
        }
    };

    let writeback_input = WritebackInput {
        channel: resource_channel,
        directory_layout,
        libsecret_state_path: if resource_channel.libsecret {
            libsecret_state_path
        } else {
            None
        },
    };

    // The authorization gates the real collection/terminal dispatch call. A
    // suppressed state never enters the fsuid window and never reads staging.
    let writeback_result = if authorization != GateAuthorization::Accepted {
        dispatch_after_gate(authorization, &[writeback_input])
    } else if needs_fsuid_window {
        target_uid::with_target_fsuid(isolation, || {
            dispatch_after_gate(authorization, &[writeback_input])
        })
        .and_then(|inner| inner)
    } else {
        dispatch_after_gate(authorization, &[writeback_input])
    };

    match writeback_result {
        Ok(Some(outcomes)) => report_outcomes(&outcomes),
        Ok(None) => {
            if !matches!(
                authorization,
                GateAuthorization::Suppressed(exec_gate::GateFailure::Failed)
            ) {
                eprintln!(
                    "underwrap-ns: end-of-run dispatch suppressed by invalid exec gate state: {authorization:?}"
                );
            }
        }
        Err(e) => eprintln!("underwrap-ns: end-of-run writeback: {e}"),
    }

    if let Err(e) = resource_channel::close_channel_fd(resource_channel) {
        eprintln!("underwrap-ns: closing resource channel after target: {e}");
    }

    if needs_fsuid_window {
        drop_caps().map_err(|e| format!("parent: dropping capabilities: {e}"))?;
    }

    propagate_exit_status(status)
}

fn dispatch_after_gate(
    authorization: GateAuthorization,
    inputs: &[WritebackInput<'_>],
) -> Result<Option<Vec<WritebackOutcome>>, String> {
    if authorization != GateAuthorization::Accepted {
        return Ok(None);
    }
    writeback::dispatch_writeback(inputs).map(Some)
}

fn wait_for_child(pid: libc::pid_t) -> Result<libc::c_int, String> {
    let mut status: libc::c_int = 0;
    loop {
        let ret = unsafe { libc::waitpid(pid, &mut status, 0) };
        if ret == pid {
            if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
                return Ok(status);
            }
            return Err(format!("waitpid returned unexpected child state {status}"));
        }
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("waitpid failed: {err}"));
        }
        return Err(format!("waitpid returned unexpected pid {ret}"));
    }
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
    let mut file = unsafe { std::fs::File::from_raw_fd(EXEC_BUNDLE_FD) };
    underwrap_protocol::ipc::read_frame_sync(&mut file).map_err(std::io::Error::other)
}
