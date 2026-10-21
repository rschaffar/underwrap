//! underwrap-ns — PID namespace isolation stage.
//!
//! First authenticates fixed fd 20, then validates and arms the private fd-21
//! stage-parent liveness link before trusting the remaining handoff. It retains
//! fd 21 through a checked pre-namespace rearm. After the secret-free idmap
//! helper, it creates the pre-exec authorization gate, an inner liveness pipe,
//! and an exact direct-target status pipe; reads fd 3; drives Checkout; and
//! completely plans the files projection.
//!
//! The host supervisor creates the namespaces and forks a trusted non-execing
//! namespace PID-1 relay. The relay arms/rearms parent-death SIGKILL around its
//! dedicated-identity switch, becomes non-dumpable, realizes files/libsecret
//! projection state with its bounded setup capabilities, then sets no-new-privs,
//! drops and verifies active capability sets, performs its final liveness check,
//! closes fd 20, and forks the configured target as PID 2 in a separate session.
//! PID 2 alone owns the gate writer. PID 1 actively polls the inner sole-writer
//! liveness pipe as the authoritative parent-lifetime check; parent-death
//! SIGKILL remains defense in depth. PID 1 reaps its direct child, frames that
//! status, and exits so the kernel tears down helpers and descendants. The host
//! authorizes collection only after exact gate acceptance, a clean relay reap,
//! and one valid status frame followed by EOF. Libsecret also requires terminal
//! publisher EOF and a strictly validated ACK-written generation. These
//! channels record an authorized lifecycle, not proof of exec; the narrow
//! pre-exec ambiguity remains unchanged.

#[cfg(test)]
mod test_support;
#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;

mod boundary;
mod caps;
mod directory_credential;
mod directory_handle;
mod env;
mod exec_gate;
mod handoff;
mod idmap;
mod journal;
mod libsecret;
mod mounts;
mod publication;
mod resource_channel;
mod supervision;
mod target_status;
mod target_uid;
mod writeback;

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use caps::{drop_caps, set_no_new_privs};
use directory_credential::{
    DirectoryMount, DirectoryProjectionPlan, plan_directory_projection, prepare_directory_mounts,
    realize_directory_mounts,
};
use env::build_target_env;
use exec_gate::{ExecGate, GateAuthorization, GateReader, GateWriter};
use libsecret::setup_libsecret;
use mounts::{make_mount_namespace_private, remount_proc, setup_mount_namespace};
use publication::{PublicationPair, Receiver};
use resource_channel::ResourceChannel;
use supervision::{LivenessWriter, SigchldState};
use target_status::{TargetStatus, TargetStatusReader, TargetStatusWriter};
use underwrap_protocol::exec::{EXEC_BUNDLE_FD, ExecBundle, IDMAP_REQUIRED_ENV};
use underwrap_protocol::resource_blob::ResourceBlob;
use writeback::{WritebackInput, WritebackOutcome};

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

    // fd 20 remains the first authority decision. Only after it authenticates
    // stage 1 do we adopt the private fd-21 liveness reader, arm the parent-death
    // link, and close the already-dead-parent race. The reader remains owned
    // through the checked pre-namespace rearm below, then is discarded before
    // the namespace fork. No selector other than the instance used for
    // daemon-account lookup, target metadata, fd 3, identity table, or
    // privileged namespace input is trusted before this point.
    let outer_liveness = supervision::adopt_outer_liveness()
        .map_err(|e| format!("adopting wrapper-parent liveness: {e}"))?;
    supervision::arm_and_check(&outer_liveness)
        .map_err(|e| format!("arming wrapper-parent lifetime: {e}"))?;

    // Each supervisor must reliably reap even when the caller inherited an
    // ignored/SA_NOCLDWAIT or blocked SIGCHLD. PID 2 restores this exact state
    // before any target-facing helper fork or configured-target exec.
    let target_sigchld = SigchldState::capture()?;
    target_sigchld.normalize_for_wait()?;

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
        // SAFETY: getgid takes no arguments and returns the real caller GID
        // before any namespace-stage credential transition.
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
    let namespace_liveness = supervision::LivenessPipe::new()?;
    let target_status = target_status::TargetStatusPipe::new()?;

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
    let host_env: std::collections::HashMap<String, String> = std::env::vars().collect();
    let target_env = build_target_env(
        &host_env,
        &exec_bundle,
        directory_plan.blob(),
        &target_identity.home,
        &xdg_runtime_dir,
    )?;
    let publication = if libsecret_enabled {
        Some(PublicationPair::new()?)
    } else {
        None
    };

    // The caller-owned directories to idmap caller<->X into the target's mount
    // namespace (`~`/`~/rel`/absolute), straight from authenticated wrapper
    // configuration in the exec bundle.
    let idmap_paths = exec_bundle.access_paths.clone();

    // Retain outer liveness through mount setup and the temporary fsid window;
    // its final checked rearm below follows every pre-fork credential change.
    supervision::arm_and_check(&outer_liveness)
        .map_err(|e| format!("checking wrapper-parent lifetime before namespace setup: {e}"))?;
    prepare_namespace()?;

    // Apply the idmapped bind mounts now: fsuid is still the caller (so open_tree
    // traverses the caller's home as owner), caps are held, and we are inside the
    // private mount namespace, so the forked child (which becomes X) inherits the
    // mounts and the host's view is untouched.
    if let Some(ref userns) = idmap_userns {
        idmap::mount_idmapped_dirs(&idmap_paths, &target_identity.home, userns)
            .map_err(|e| format!("setting up idmapped directories: {e}"))?;
    }

    // Prepare only empty projection directories before fork, giving both
    // supervisors handles to the same filesystem objects. The tmpfs is attached
    // through a checked target-owned runtime directory only in this namespace.
    let directory_layout = if has_files {
        let layout = target_uid::with_target_fsuid(isolation, || {
            // Filesystem-ID changes can reset dumpability. Reestablish the
            // invariant before setup while decoded brokered bytes are live.
            make_process_non_dumpable()?;
            prepare_directory_mounts(&directory_plan, Path::new(&xdg_runtime_dir), isolation)
        });
        // Restoration can reset it again; check even when setup failed.
        make_process_non_dumpable()?;
        layout??
    } else {
        Vec::new()
    };
    supervision::arm_and_check(&outer_liveness)
        .map_err(|e| format!("rearming wrapper-parent lifetime before namespace fork: {e}"))?;
    drop(outer_liveness);

    let pid = fork_into_namespace()?;
    if pid == 0 {
        let publisher = publication.map(|pair| {
            drop(pair.host);
            pair.publisher
        });
        run_relay(
            exec_gate.into_child(),
            namespace_liveness.into_relay(),
            target_status.into_relay(),
            target_sigchld,
            target,
            target_args,
            &target_identity,
            target_env,
            &resource_channel,
            libsecret_enabled,
            publisher,
            isolation,
            has_files,
            &directory_plan,
            directory_layout,
        )
    } else {
        let receiver = publication.map(|pair| {
            drop(pair.publisher);
            pair.host
        });
        run_parent(
            pid,
            exec_gate.into_parent(),
            namespace_liveness.into_host(),
            target_status.into_host(),
            &directory_layout,
            &resource_channel,
            has_files,
            isolation,
            receiver,
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
    // SAFETY: PR_SET_DUMPABLE takes trusted scalar arguments and changes only
    // calling-process state; the return is checked before handoff bytes are read.
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
    // SAFETY: `flags` combines only trusted namespace constants and the process
    // still has the required capability; the return is checked before forking.
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
        &resource_channel.resource_id,
        blob,
    )
    .map_err(|e| format!("planning complete files projection: {e}"))
}

/// Realize only a completed files projection after switching to the dedicated
/// target uid, so fsuid is naturally X.
fn realize_files_mounts(
    target_home: &Path,
    plan: &DirectoryProjectionPlan,
    mounts: &[DirectoryMount],
) -> Result<(), String> {
    if plan.is_empty() {
        return Ok(());
    }
    realize_directory_mounts(target_home, plan, mounts)
        .map_err(|e| format!("realizing files-slot mounts: {e}"))
}

fn fork_into_namespace() -> Result<libc::pid_t, String> {
    // SAFETY: underwrap-ns is single-threaded here; each branch immediately
    // follows the designated owned-fd relay or host-supervisor path.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("fork failed: {err}"));
    }
    Ok(pid)
}

#[allow(clippy::too_many_arguments)]
fn run_relay(
    mut gate_writer: GateWriter,
    liveness_reader: supervision::LivenessReader,
    status_writer: TargetStatusWriter,
    target_sigchld: SigchldState,
    target: String,
    target_args: Vec<String>,
    target_identity: &TargetIdentity,
    target_env: std::collections::HashMap<String, String>,
    resource_channel: &ResourceChannel,
    libsecret_enabled: bool,
    mut publisher: Option<std::os::unix::net::UnixStream>,
    isolation: target_uid::Isolation,
    has_files: bool,
    directory_plan: &DirectoryProjectionPlan,
    directory_layout: Vec<DirectoryMount>,
) -> ! {
    let prepared = (|| -> Result<_, String> {
        // The host owns the sole writer. Arm before namespace-local setup as
        // defense in depth; retaining the reader closes already-dead-parent
        // races through each checked rearm and later authoritative polling.
        supervision::arm_and_check(&liveness_reader)?;

        remount_proc().map_err(|e| format!("remounting /proc failed: {e}"))?;
        target_uid::switch_to(isolation)
            .map_err(|e| format!("switching to dedicated target uid: {e}"))?;

        // Credential changes clear PDEATHSIG. Rearm/check immediately before
        // any staging, helper, or PID-2 target fork.
        supervision::arm_and_check(&liveness_reader)
            .map_err(|e| format!("rearming namespace lifetime after credential switch: {e}"))?;
        make_process_non_dumpable()?;

        if has_files {
            realize_files_mounts(&target_identity.home, directory_plan, &directory_layout)?;
        }
        // Only the host collector retains staging authority. Drop the relay's
        // handles before any target/helper fork; all handles are also CLOEXEC.
        drop(directory_layout);
        if libsecret_enabled {
            setup_mount_namespace().map_err(|e| format!("mount namespace setup failed: {e}"))?;
        }

        set_no_new_privs().map_err(|e| format!("setting no_new_privs: {e}"))?;
        boundary::join_session_keyring().map_err(|e| format!("joining session keyring: {e}"))?;
        drop_caps().map_err(|e| format!("dropping capabilities: {e}"))?;
        target_uid::verify_switched().map_err(|e| format!("verifying target uid switch: {e}"))?;
        disable_core_dumps()?;
        make_process_non_dumpable()?;
        boundary::sever_controlling_tty()
            .map_err(|e| format!("severing relay controlling tty: {e}"))?;

        let mut cmd = std::process::Command::new(&target);
        cmd.args(&target_args);
        cmd.env_clear();
        let helper_env: std::collections::HashMap<String, String> =
            ["HOME", "PATH", "LANG", "LC_ALL", "TZ"]
                .iter()
                .filter_map(|key| {
                    target_env
                        .get(*key)
                        .map(|value| ((*key).to_string(), value.clone()))
                })
                .collect();
        let libsecret_bytes = if libsecret_enabled {
            Some(libsecret_section_bytes(
                resource_channel,
                directory_plan.blob(),
            )?)
        } else {
            None
        };

        // Capability and related credential transitions can clear PDEATHSIG.
        // Keep the private reader through all relay setup, then perform the
        // final race-closed arm/check immediately before fd-20 closure and the
        // target fork. PID 1 retains the reader because polling the sole-writer
        // pipe is the authoritative parent-lifetime check; PDEATHSIG is defense
        // in depth. PID 2 closes its inherited copy before target-visible work.
        supervision::arm_and_check(&liveness_reader)
            .map_err(|e| format!("final rearm of namespace lifetime: {e}"))?;

        // Relay and target must not hold the authenticated daemon capability.
        // The host has an independent process copy for finalization.
        resource_channel::close_channel_fd(resource_channel)
            .map_err(|e| format!("closing resource channel before target fork: {e}"))?;
        Ok((cmd, target_env, helper_env, libsecret_bytes))
    })();

    let (mut cmd, target_env, helper_env, libsecret_bytes) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            if let Err(gate_error) = gate_writer.send_failed() {
                eprintln!("underwrap-ns: {gate_error}");
            }
            eprintln!("underwrap-ns: {error}");
            // SAFETY: post-fork relay setup failure must not unwind host state.
            unsafe { libc::_exit(1) }
        }
    };

    // SAFETY: the relay is single-threaded and performs only owned-fd/process
    // operations before the PID-2 branch execs or exits via _exit.
    let target_pid = unsafe { libc::fork() };
    if target_pid < 0 {
        let error = format!(
            "forking configured target: {}",
            std::io::Error::last_os_error()
        );
        if let Err(gate_error) = gate_writer.send_failed() {
            eprintln!("underwrap-ns: {gate_error}");
        }
        eprintln!("underwrap-ns: {error}");
        // SAFETY: no PID 2 exists and this post-fork relay must not unwind.
        unsafe { libc::_exit(1) }
    }

    if target_pid == 0 {
        // First branch action: descendants and helpers must never inherit the
        // relay-only target-status writer.
        drop(status_writer);
        drop(liveness_reader);
        if let Err(error) = run_target(
            &mut gate_writer,
            &target_sigchld,
            &mut cmd,
            target_env,
            &helper_env,
            libsecret_bytes.as_deref(),
            publisher.take(),
            libsecret_enabled,
        ) {
            if let Err(gate_error) = gate_writer.send_failed() {
                eprintln!("underwrap-ns: {gate_error}");
            }
            eprintln!("underwrap-ns: {error}");
        }
        // SAFETY: PID 2 controlled-return path must not unwind relay state.
        unsafe { libc::_exit(1) }
    }

    // PID 1 must not retain publication authority after the PID-2 setup branch
    // exists. Closing its gate copy makes EOF reflect PID 2 alone.
    drop(publisher);
    drop(gate_writer);
    let raw_status = match wait_for_target_with_liveness(target_pid, &liveness_reader) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("underwrap-ns: relay waiting for target: {error}");
            // SAFETY: an unclean relay exit is intentionally fail-closed and
            // causes kernel SIGKILL of all remaining namespace members.
            unsafe { libc::_exit(1) }
        }
    };
    let status = match TargetStatus::from_wait_status(raw_status) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("underwrap-ns: {error}");
            // SAFETY: PID 1 cannot encode a valid direct-child status; `_exit`
            // terminates without unwinding duplicated post-fork process state.
            unsafe { libc::_exit(1) }
        }
    };
    if let Err(error) = status_writer.write(status) {
        eprintln!("underwrap-ns: {error}");
        // SAFETY: PID 1 failed its sole-writer status handoff; `_exit` avoids
        // unwinding inherited state and makes the relay reap unclean.
        unsafe { libc::_exit(1) }
    }
    // Exiting namespace init makes the kernel SIGKILL every helper or detached
    // descendant before the host can reap this relay.
    // SAFETY: the status frame was written successfully; `_exit` terminates PID
    // 1 directly so namespace teardown, rather than Rust unwinding, follows.
    unsafe { libc::_exit(0) }
}

#[allow(clippy::too_many_arguments)]
fn run_target(
    gate_writer: &mut GateWriter,
    target_sigchld: &SigchldState,
    cmd: &mut std::process::Command,
    target_env: std::collections::HashMap<String, String>,
    helper_env: &std::collections::HashMap<String, String>,
    libsecret_bytes: Option<&[u8]>,
    publisher: Option<std::os::unix::net::UnixStream>,
    libsecret_enabled: bool,
) -> Result<(), String> {
    target_sigchld.restore()?;
    boundary::sever_controlling_tty()
        .map_err(|e| format!("severing target controlling tty: {e}"))?;

    if libsecret_enabled {
        let publisher = publisher
            .ok_or_else(|| "libsecret enabled but no publisher endpoint provided".to_string())?;
        let section_bytes = libsecret_bytes
            .ok_or_else(|| "libsecret enabled but no initial section provided".to_string())?;
        setup_libsecret(
            cmd,
            gate_writer.raw_fd(),
            helper_env,
            section_bytes,
            publisher,
        )?;
    } else {
        drop(publisher);
    }
    for (key, value) in target_env {
        cmd.env(key, value);
    }

    // Every internal descriptor is explicitly owned/closed before this final
    // backstop; CLOEXEC then protects against an accidental future descriptor.
    boundary::close_inherited_fds_for_exec()
        .map_err(|e| format!("protecting inherited fds before exec: {e}"))?;
    gate_writer.send_armed()?;
    let error = cmd.exec();
    Err(format!("exec failed: {error}"))
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
    // SAFETY: `rlim` is initialized, aligned, and live for the call; RLIMIT_CORE
    // is trusted and the return is checked before configured-target exec.
    let ret = unsafe { libc::setrlimit(libc::RLIMIT_CORE, &rlim) };
    if ret != 0 {
        return Err(format!(
            "setrlimit(RLIMIT_CORE, 0) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_parent(
    pid: libc::pid_t,
    gate_reader: GateReader,
    namespace_liveness: LivenessWriter,
    target_status_reader: TargetStatusReader,
    directory_layout: &[DirectoryMount],
    resource_channel: &ResourceChannel,
    has_files: bool,
    isolation: target_uid::Isolation,
    mut publication_receiver: Option<Receiver>,
) -> Result<(), String> {
    debug_assert!(namespace_liveness.raw_fd() >= 0);
    let needs_fsuid_window = has_files;
    if !needs_fsuid_window {
        drop_caps().map_err(|e| format!("parent: dropping capabilities: {e}"))?;
    }

    let publication_enabled = publication_receiver.is_some();
    if let Some(receiver) = publication_receiver.as_mut() {
        let checkout = ResourceBlob::from_canonical_bytes(&resource_channel.original_canonical)
            .map_err(|error| format!("decoding Checkout baseline for publication: {error}"))?;
        receiver.set_composition_baseline(&checkout);
    }

    // Generation zero is synchronously decided before the unchanged blocking
    // gate parser. secretsd cannot report ready, and therefore the target cannot
    // publish a mutation, until ACK(0) has been completely written.
    let initial_publication = if let Some(receiver) = publication_receiver.as_mut() {
        receiver.receive_initial()
    } else {
        Ok(())
    };
    if let Err(error) = &initial_publication {
        eprintln!("underwrap-ns: initial libsecret publication failed: {error}");
    }

    // Gate EOF is observed first. The sole inner writer stays live until the
    // relay is reaped, so any host-supervisor death before then kills PID 1.
    let authorization = gate_reader.observe();
    let relay_status = match match publication_receiver.as_mut() {
        Some(receiver) => publication::wait_for_relay_with_publication(pid, receiver),
        None => wait_for_child(pid),
    } {
        Ok(status) => status,
        Err(error) => {
            drop(namespace_liveness);
            return parent_supervisor_failure(
                resource_channel,
                needs_fsuid_window,
                format!("relay wait failed: {error}"),
            );
        }
    };
    drop(namespace_liveness);

    if !libc::WIFEXITED(relay_status) || libc::WEXITSTATUS(relay_status) != 0 {
        return parent_supervisor_failure(
            resource_channel,
            needs_fsuid_window,
            format!("namespace relay exited uncleanly with wait status {relay_status}"),
        );
    }
    let target_status = match target_status_reader.read_after_clean_relay() {
        Ok(status) => status,
        Err(error) => {
            return parent_supervisor_failure(
                resource_channel,
                needs_fsuid_window,
                format!("invalid direct-target status after clean relay reap: {error}"),
            );
        }
    };

    // After clean reap, one final nonblocking drain distinguishes terminal EOF
    // from a retained publisher endpoint. WouldBlock fails closed immediately.
    let terminal_publication = if let Some(receiver) = publication_receiver.as_mut() {
        match receiver.drain_available() {
            Ok(would_block) => receiver.terminal_generation(would_block).map(Some),
            Err(error) => Err(error),
        }
    } else {
        Ok(None)
    };
    if let Err(error) = &terminal_publication {
        eprintln!("underwrap-ns: libsecret terminal publication invalid: {error}");
    }
    let selected_generation = terminal_publication.as_ref().ok().and_then(Option::as_ref);
    let _selected_publication_generation =
        selected_generation.map(|generation| generation.generation);

    let selected_libsecret = selected_generation.map(|generation| &generation.section);
    let writeback_input = WritebackInput {
        channel: resource_channel,
        directory_layout,
        libsecret_generation: selected_libsecret,
    };

    // Dispatch requires the authorized exec gate, valid direct-target status,
    // clean relay reap, and valid terminal publication for libsecret.
    // Suppression never replaces exact target status.
    let publication_authorized =
        !publication_enabled || (initial_publication.is_ok() && selected_generation.is_some());
    let effective_authorization = if publication_authorized {
        authorization
    } else {
        GateAuthorization::Suppressed(exec_gate::GateFailure::Failed)
    };
    let writeback_result = if effective_authorization != GateAuthorization::Accepted {
        dispatch_after_gate(effective_authorization, &[writeback_input])
    } else if needs_fsuid_window {
        target_uid::with_target_fsuid(isolation, || {
            dispatch_after_gate(effective_authorization, &[writeback_input])
        })
        .and_then(|inner| inner)
    } else {
        dispatch_after_gate(effective_authorization, &[writeback_input])
    };

    match writeback_result {
        Ok(Some(outcomes)) => report_outcomes(&outcomes),
        Ok(None) => {
            if !matches!(
                effective_authorization,
                GateAuthorization::Suppressed(exec_gate::GateFailure::Failed)
            ) {
                eprintln!(
                    "underwrap-ns: end-of-run dispatch suppressed by invalid exec gate state: {effective_authorization:?}"
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
    propagate_target_status(target_status)
}

fn parent_supervisor_failure(
    resource_channel: &ResourceChannel,
    needs_fsuid_window: bool,
    reason: String,
) -> Result<(), String> {
    let close_error = resource_channel::close_channel_fd(resource_channel).err();
    let drop_error = if needs_fsuid_window {
        drop_caps()
            .err()
            .map(|error| format!("parent: dropping capabilities: {error}"))
    } else {
        None
    };
    Err(format!(
        "{reason}{}{}",
        close_error
            .map(|error| format!("; closing resource channel: {error}"))
            .unwrap_or_default(),
        drop_error
            .map(|error| format!("; {error}"))
            .unwrap_or_default()
    ))
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

fn wait_for_target_with_liveness(
    pid: libc::pid_t,
    liveness_reader: &supervision::LivenessReader,
) -> Result<libc::c_int, String> {
    loop {
        // A short blocking poll avoids busy-spinning while bounding cancellation
        // observation. Any event is invalid because the sole writer never sends
        // application data.
        supervision::poll_parent_alive(liveness_reader, 50)?;
        let mut status = 0;
        // SAFETY: status is writable and pid names the direct PID-2 child.
        let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if result == 0 {
            continue;
        }
        if result == pid {
            if !libc::WIFEXITED(status) && !libc::WIFSIGNALED(status) {
                return Err(format!("waitpid returned unexpected target state {status}"));
            }
            // Close the race between the final poll and accepting/framing the
            // target status. This also restores PDEATHSIG defense in depth.
            supervision::arm_and_check(liveness_reader)?;
            return Ok(status);
        }
        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("waitpid for direct target failed: {error}"));
        }
        return Err(format!("waitpid returned unexpected target pid {result}"));
    }
}

fn wait_for_child(pid: libc::pid_t) -> Result<libc::c_int, String> {
    let mut status: libc::c_int = 0;
    loop {
        // SAFETY: `pid` names the direct child and `status` is aligned writable
        // storage; only a matching successful return permits status inspection.
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
/// emitted when a writeback is dropped because the permission is read-only.
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
    let suppress_readonly_warning = read_only_warning_suppressed();
    for outcome in outcomes {
        match outcome {
            WritebackOutcome::Unchanged | WritebackOutcome::Persisted => continue,
            // Wrapper-local decision: the Checkout's writable flag was
            // already false, so no Checkin was attempted.
            WritebackOutcome::DroppedReadOnly {
                resource_id,
                resource_name,
                usage_error,
            } => {
                emit_audit_event("permission_readonly", resource_id, resource_name, None);
                report_usage_failure(resource_id, resource_name, usage_error.as_ref());
                if !suppress_readonly_warning {
                    eprintln!(
                        "underwrap: changes to resource '{}' ({}) were not persisted \
                         (permission is read-only). Set {SUPPRESS_READONLY_WARNING_ENV}=1 to \
                         silence this message.",
                        one_line_display(resource_name),
                        one_line_display(resource_id),
                    );
                }
            }
            // Server-side rejection: Checkin was sent but backend finalize
            // refused it (for example, the permission became read-only mid-run).
            WritebackOutcome::RejectedReadOnly {
                resource_id,
                resource_name,
                usage_error,
            } => {
                emit_audit_event("server_rejected", resource_id, resource_name, None);
                report_usage_failure(resource_id, resource_name, usage_error.as_ref());
                if !suppress_readonly_warning {
                    eprintln!(
                        "underwrap: changes to resource '{}' ({}) were not persisted \
                         (permission is read-only). Set {SUPPRESS_READONLY_WARNING_ENV}=1 to \
                         silence this message.",
                        one_line_display(resource_name),
                        one_line_display(resource_id),
                    );
                }
            }
            WritebackOutcome::RejectedTooLarge {
                resource_id,
                resource_name,
                attempted_len,
                maximum_len,
            } => {
                emit_audit_event(
                    "resource_too_large",
                    resource_id,
                    resource_name,
                    Some((*attempted_len, *maximum_len)),
                );
                // This is not a read-only warning: a caller cannot suppress a
                // rejected complete-resource composition diagnostic.
                eprintln!(
                    "underwrap-ns: end-of-run writeback: post-run resource for '{}' \
                     exceeds the complete resource size limit: resource blob exceeds \
                     maximum of {maximum_len} bytes (got {attempted_len}); resource name \
                     '{}'; reason=resource_too_large",
                    one_line_display(resource_id),
                    one_line_display(resource_name),
                );
            }
        }
    }
}

fn report_usage_failure(
    resource_id: &str,
    resource_name: &str,
    error: Option<&writeback::UsageReportFailure>,
) {
    if let Some(error) = error {
        eprintln!(
            "underwrap-ns: end-of-run usage reporting for resource '{}' ({}): {error}",
            one_line_display(resource_name),
            one_line_display(resource_id),
        );
    }
}

/// Escape one UTF-8 string as a JSON string without a serialization
/// dependency. JSON permits non-ASCII UTF-8 directly; quotes, backslashes,
/// standard controls, and every remaining C0 control are escaped.
fn json_string(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() + 2);
    encoded.push('"');
    for character in value.chars() {
        match character {
            '"' => encoded.push_str("\\\""),
            '\\' => encoded.push_str("\\\\"),
            '\u{08}' => encoded.push_str("\\b"),
            '\u{0C}' => encoded.push_str("\\f"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            character if character <= '\u{1F}' => {
                use std::fmt::Write as _;
                write!(encoded, "\\u{:04x}", character as u32).expect("writing to String");
            }
            character => encoded.push(character),
        }
    }
    encoded.push('"');
    encoded
}

/// Render metadata in one physical display line. Ordinary safe names retain
/// their existing text; quotes, backslashes, and controls cannot alter the
/// surrounding warning or forge an additional terminal line.
fn one_line_display(value: &str) -> String {
    let encoded = json_string(value);
    encoded[1..encoded.len() - 1].to_string()
}

fn audit_json_line(
    reason: &str,
    resource_id: &str,
    resource_name: &str,
    size: Option<(usize, usize)>,
) -> String {
    let mut line = format!(
        r#"{{"audit":"writeback_rejected","reason":{},"resource_id":{},"resource_name":{}}}"#,
        json_string(reason),
        json_string(resource_id),
        json_string(resource_name),
    );
    if let Some((attempted_len, maximum_len)) = size {
        line.pop();
        line.push_str(&format!(
            ",\"maximum_length\":{maximum_len},\"attempted_length\":{attempted_len}}}"
        ));
    }
    line
}

/// Send the `writeback_rejected` event to journald. On non-systemd
/// hosts (or if the journal socket send fails), fall back to a valid one-line
/// stderr JSON record so the audit trail is not lost on dev / test runs.
fn emit_audit_event(
    reason: &str,
    resource_id: &str,
    resource_name: &str,
    size: Option<(usize, usize)>,
) {
    let (message, maximum_length, attempted_length) = match size {
        Some((attempted_len, maximum_len)) => (
            format!(
                "writeback rejected for resource '{}' ({}): {reason}; \
                 maximum_length={maximum_len}; attempted_length={attempted_len}",
                one_line_display(resource_name),
                one_line_display(resource_id),
            ),
            Some(maximum_len.to_string()),
            Some(attempted_len.to_string()),
        ),
        None => (
            format!(
                "writeback rejected for resource '{}' ({}): {reason}",
                one_line_display(resource_name),
                one_line_display(resource_id),
            ),
            None,
            None,
        ),
    };
    let mut fields = vec![
        ("MESSAGE", message.as_str()),
        ("PRIORITY", JOURNAL_PRIORITY_WARNING),
        ("SYSLOG_IDENTIFIER", JOURNAL_SYSLOG_IDENTIFIER),
        ("AUDIT", "writeback_rejected"),
        ("REASON", reason),
        ("RESOURCE_ID", resource_id),
        ("RESOURCE_NAME", resource_name),
    ];
    if let (Some(maximum_len), Some(attempted_len)) =
        (maximum_length.as_deref(), attempted_length.as_deref())
    {
        fields.push(("MAXIMUM_LENGTH", maximum_len));
        fields.push(("ATTEMPTED_LENGTH", attempted_len));
    }
    if journal::send_record(&fields).is_err() {
        // The journal field names map 1:1 to the JSON keys so log pipelines
        // can be written once. `audit_json_line` keeps arbitrary UTF-8
        // metadata valid JSON and physically confined to this one line.
        eprintln!(
            "{}",
            audit_json_line(reason, resource_id, resource_name, size)
        );
    }
}

fn propagate_target_status(status: TargetStatus) -> Result<(), String> {
    match status {
        TargetStatus::Exited(code) => std::process::exit(code.into()),
        TargetStatus::Signaled(signal) => supervision::propagate_signal(signal),
    }
}

fn resolve_target_identity() -> Result<TargetIdentity, std::io::Error> {
    // SAFETY: getuid takes no arguments and returns the real caller UID before
    // the dedicated target-identity switch.
    let uid = unsafe { libc::getuid() };
    // SAFETY: getpwuid takes a scalar UID. This process is single-threaded and
    // makes no concurrent NSS lookup; the static result is checked before use.
    let pwd = unsafe { libc::getpwuid(uid) };
    if pwd.is_null() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("getpwuid({uid}) returned null"),
        ));
    }
    // SAFETY: `pwd` is non-null and no intervening NSS call has replaced its
    // static storage; `pw_dir` is an initialized NUL-terminated string copied now.
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

    // SAFETY: after fd-20 peer authentication, fixed fd 3 is the live pipe
    // read-end passed by underwrap-exec. This function runs once and transfers
    // its sole process-local ownership into `File` for exact RAII closure.
    let mut file = unsafe { std::fs::File::from_raw_fd(EXEC_BUNDLE_FD) };
    underwrap_protocol::ipc::read_frame_sync(&mut file).map_err(std::io::Error::other)
}
