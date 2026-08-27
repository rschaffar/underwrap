//! underwrap-exec — the per-instance wrapper binary (stage 1).
//!
//! One setgid binary serves a whole instance. It is installed (via NixOS
//! `security.wrappers`) as `underwrap-<instance>` with setgid to the single
//! `underwrap-<instance>` group, and invoked as:
//!
//! ```text
//! underwrap-<instance> <wrapper> [args...]          # normal invocation
//! underwrap-<instance> debug <wrapper> [args...]    # debug variant
//! ```
//!
//! 1. Verifies instance identity from the effective GID group
//!    (`underwrap-<instance>`), never from argv[0]
//! 2. Parses the wrapper name + variant from argv; the daemon validates
//!    the name and the backend authorises the variant
//! 3. Loads the trusted instance sidecar (socket + helper binary paths)
//! 4. Connects to the instance socket and sends `Read { wrapper_name,
//!    variant, scope }`; receives projection metadata, the daemon-chosen
//!    target binary, and one wrapper-resource channel fd via SCM_RIGHTS
//! 5. Drops all elevated privileges
//! 6. Relocates every child-carried descriptor above the reserved slots, then
//!    forks: the waiting parent writes the descriptor-free `ExecBundle` to fd 3
//!    and owns the sole liveness writer; the child maps the daemon-created socket
//!    to fd 20 and the liveness reader to fd 21 before execing `underwrap-ns`
//! 7. `underwrap-ns` authenticates fd 20's creation-time daemon credentials,
//!    then validates/arms fd 21 before reading fd 3, setting up namespaces, or
//!    trusting target metadata
//!
//! The wrapper never chooses the target binary: it execs whatever path
//! the daemon returned in `ReadOk`. fd 3 carries only descriptor-free execution
//! metadata; brokered resource bytes flow over the authenticated fd-20 channel,
//! never argv or an inter-stage environment variable.

mod supervision;

use anyhow::{Context, Result, bail};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::exec::{
    EXEC_BUNDLE_FD, ExecBundle, ExecResourceBinding, IDMAP_REQUIRED_ENV,
    WRAPPER_PARENT_LIVENESS_FD, WRAPPER_RESOURCE_CHANNEL_FD,
};
use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{WrapperRequest, WrapperResponse};
use underwrap_protocol::variant::WrapperVariant;

/// Fixed directory holding one trusted sidecar per instance. No
/// environment override — that would allow config redirection when the
/// binary is run without the setgid wrapper.
const TRUSTED_INSTANCE_CONFIG_DIR: &str = "/etc/underwrap/instances";

/// Path to the underwrap-ns binary when the instance sidecar omits one.
const DEFAULT_NS_BINARY: &str = "/run/wrappers/bin/underwrap-ns";

/// The prefix used by the NixOS module for the per-instance group name.
const GROUP_PREFIX: &str = "underwrap-";

/// Debug subcommand token: `underwrap-<instance> debug <wrapper>`.
const DEBUG_SUBCOMMAND: &str = "debug";

/// One parsed invocation: which wrapper, which variant, and the residual
/// argv to hand the target.
#[derive(Debug)]
struct Invocation {
    variant: WrapperVariant,
    wrapper_name: String,
    target_args: Vec<String>,
}

/// Everything the daemon returned for this invocation, plus the received
/// channel fd. `target` is the daemon-chosen binary — the wrapper never
/// picks it.
struct ReadResult {
    env_projection: bool,
    target: String,
    env_passthrough: Vec<String>,
    env_set: std::collections::HashMap<String, String>,
    target_runtime_dir_base: String,
    access_paths: Vec<String>,
    resource_binding: ReceivedResourceBinding,
}

struct ReceivedResourceBinding {
    resource_id: String,
    resource_name: String,
    access: AccessMode,
    libsecret: bool,
    files_target_paths: Vec<String>,
    channel_fd: OwnedFd,
}

fn main() -> Result<()> {
    // Instance identity comes exclusively from the kernel-assigned
    // effective GID group (`underwrap-<instance>`), never from argv[0].
    let instance = instance_from_egid()?;
    let invocation = parse_invocation(&std::env::args().skip(1).collect::<Vec<_>>())?;
    let instance_config = load_instance_config(&instance)?;

    let read = request_secret(
        &instance_config.socket,
        &invocation.wrapper_name,
        invocation.variant,
    )?;

    // Drop all elevated privileges before touching the target binary.
    drop_privilege()?;

    let ns_binary = instance_config
        .ns_binary
        .as_deref()
        .unwrap_or(DEFAULT_NS_BINARY);
    if !ns_binary.starts_with('/') {
        bail!("ns_binary must be an absolute path, got: {ns_binary:?}");
    }
    if !std::path::Path::new(&read.target).is_absolute() {
        bail!(
            "daemon returned a non-absolute target path: {:?}",
            read.target
        );
    }

    exec_via_ns(
        ns_binary,
        &instance_config,
        &instance,
        &invocation.wrapper_name,
        &invocation.target_args,
        read,
    )
}

/// Parse `[debug] <wrapper> [args...]`. The leading `debug` token selects
/// the debug variant; otherwise the first argument is the wrapper name.
fn parse_invocation(args: &[String]) -> Result<Invocation> {
    let mut iter = args.iter();
    let first = iter.next().context(
        "usage: underwrap-<instance> [debug] <wrapper> [args...]\n\
         missing wrapper name",
    )?;

    let (variant, wrapper_name) = if first == DEBUG_SUBCOMMAND {
        let wrapper = iter.next().context(
            "usage: underwrap-<instance> debug <wrapper> [args...]\n\
             missing wrapper name after 'debug'",
        )?;
        (WrapperVariant::Debug, wrapper.clone())
    } else {
        (WrapperVariant::Real, first.clone())
    };

    Ok(Invocation {
        variant,
        wrapper_name,
        target_args: iter.cloned().collect(),
    })
}

/// Derive the instance name from the effective GID group. The group is
/// `underwrap-<instance>`; acquiring it (via setgid) is the sole
/// kernel-enforced gate, so it is the only trusted identity source.
fn instance_from_egid() -> Result<String> {
    let egid = unsafe { libc::getegid() };
    let group_name = resolve_gid_name(egid)
        .with_context(|| format!("cannot resolve effective GID {egid} to a group name"))?;

    let instance = group_name.strip_prefix(GROUP_PREFIX).with_context(|| {
        format!(
            "effective GID group '{group_name}' does not start with '{GROUP_PREFIX}' — \
             not an underwrap instance group"
        )
    })?;
    if instance.is_empty() {
        bail!("effective GID group '{group_name}' has an empty instance suffix");
    }
    Ok(instance.to_string())
}

/// Resolve a GID to a group name via getgrgid.
fn resolve_gid_name(gid: u32) -> Result<String> {
    let grp = unsafe { libc::getgrgid(gid) };
    if grp.is_null() {
        bail!("getgrgid({gid}) returned null");
    }
    let name = unsafe { std::ffi::CStr::from_ptr((*grp).gr_name) };
    Ok(name
        .to_str()
        .context("group name is not valid UTF-8")?
        .to_string())
}

#[derive(Debug, PartialEq, Eq)]
struct InstanceConfig {
    /// Instance wrapper socket the daemon listens on.
    socket: String,
    /// underwrap-ns binary path; falls back to [`DEFAULT_NS_BINARY`].
    ns_binary: Option<String>,
    /// dbus-daemon binary path (libsecret mode).
    dbus_daemon_binary: Option<String>,
    /// underwrap-secretsd binary path (libsecret mode).
    secretsd_binary: Option<String>,
}

fn parse_instance_config_text(text: &str) -> Result<InstanceConfig> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct InstanceFile {
        socket: String,
        #[serde(default)]
        ns_binary: Option<String>,
        #[serde(default)]
        dbus_daemon_binary: Option<String>,
        #[serde(default)]
        secretsd_binary: Option<String>,
    }

    let f: InstanceFile = serde_yaml::from_str(text)?;
    if !std::path::Path::new(&f.socket).is_absolute() {
        bail!(
            "instance socket must be an absolute path, got: {:?}",
            f.socket
        );
    }
    Ok(InstanceConfig {
        socket: f.socket,
        ns_binary: f.ns_binary,
        dbus_daemon_binary: f.dbus_daemon_binary,
        secretsd_binary: f.secretsd_binary,
    })
}

fn load_instance_config(instance: &str) -> Result<InstanceConfig> {
    let path = format!("{TRUSTED_INSTANCE_CONFIG_DIR}/{instance}.yaml");
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading instance config {path}"))?;
    parse_instance_config_text(&text)
}

/// Request credentials from the daemon. Sends the client-supplied
/// wrapper name + variant; the daemon validates the name, the backend
/// authorises the variant, and the daemon returns the exec plan.
fn request_secret(
    socket_path: &str,
    wrapper_name: &str,
    variant: WrapperVariant,
) -> Result<ReadResult> {
    let mut stream = UnixStream::connect(socket_path)
        .with_context(|| format!("connecting to daemon at {socket_path}"))?;

    let scope = std::env::var("UNDERWRAP_SCOPE").ok();
    let req = WrapperRequest::Read {
        wrapper_name: wrapper_name.to_string(),
        variant,
        scope,
    };
    write_frame_sync(&mut stream, &req)?;

    let response: WrapperResponse = read_frame_sync(&mut stream)?;

    let (
        env_projection,
        resource_id,
        resource_name,
        access,
        libsecret,
        files_target_paths,
        target,
        env_passthrough,
        env_set,
        target_runtime_dir_base,
        access_paths,
    ) = match response {
        WrapperResponse::ReadOk {
            env_projection,
            resource_id,
            resource_name,
            access,
            libsecret,
            files_target_paths,
            target,
            env_passthrough,
            env_set,
            target_runtime_dir_base,
            access_paths,
        } => (
            env_projection,
            resource_id,
            resource_name,
            access,
            libsecret,
            files_target_paths,
            target,
            env_passthrough,
            env_set,
            target_runtime_dir_base,
            access_paths,
        ),
        WrapperResponse::Error { code, message } => {
            if matches!(code, underwrap_protocol::message::ErrorCode::StoreLocked) {
                bail!(
                    "underwrap: backing store is locked; ask an administrator to unlock it ({message})"
                );
            }
            bail!("daemon denied request ({code:?}): {message}");
        }
    };

    let fd = underwrap_protocol::scm_rights::recv_fd(stream.as_fd()).with_context(|| {
        format!("failed to receive wrapper-resource channel fd for '{resource_id}'")
    })?;

    Ok(ReadResult {
        env_projection,
        target: target.to_string_lossy().into_owned(),
        env_passthrough,
        env_set,
        target_runtime_dir_base,
        access_paths,
        resource_binding: ReceivedResourceBinding {
            resource_id,
            resource_name,
            access,
            libsecret,
            files_target_paths,
            channel_fd: fd,
        },
    })
}

/// Move the received resource channel away from both reserved handoff slots
/// before creating the fd-3 pipe. This prevents descriptor-allocation order
/// from letting the first `dup2` clobber the channel. `F_DUPFD_CLOEXEC` keeps the
/// temporary copy out of an accidental exec; the deliberate `dup2` to fd 20
/// clears close-on-exec on the fixed destination.
fn relocate_resource_channel_fd(fd: OwnedFd) -> Result<OwnedFd> {
    let source = fd.as_raw_fd();
    // SAFETY: source is held open by fd; F_DUPFD_CLOEXEC creates a distinct
    // descriptor at or above the reserved handoff slots without borrowing memory.
    let relocated =
        unsafe { libc::fcntl(source, libc::F_DUPFD_CLOEXEC, supervision::RELOCATED_FD_MIN) };
    if relocated < 0 {
        bail!(
            "relocating wrapper-resource channel fd {source}: {}",
            std::io::Error::last_os_error()
        );
    }
    // SAFETY: successful F_DUPFD_CLOEXEC returned one new uniquely owned fd.
    let relocated = unsafe { OwnedFd::from_raw_fd(relocated) };
    drop(fd);
    Ok(relocated)
}

/// Fork, pipe ExecBundle to the child, child execs underwrap-ns.
fn exec_via_ns(
    ns_binary: &str,
    config: &InstanceConfig,
    instance: &str,
    wrapper: &str,
    args: &[String],
    read: ReadResult,
) -> Result<()> {
    if !std::path::Path::new(ns_binary).exists() {
        bail!(
            "underwrap-ns not found at {ns_binary}. \
             PID namespace isolation requires underwrap-ns with cap_sys_admin."
        );
    }

    // Make this process non-dumpable. Since stage 1 waits rather than execing,
    // the setting protects its in-memory handoff state for the full invocation.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) } != 0 {
        bail!(
            "prctl(PR_SET_DUMPABLE, 0) failed: {}",
            std::io::Error::last_os_error()
        );
    }

    let resource_binding = read.resource_binding;
    let resource_source = relocate_resource_channel_fd(resource_binding.channel_fd)?;

    let mut bundle_fds = [-1; 2];
    // SAFETY: the array has room for both pipe2 results; successful descriptors
    // are adopted exactly once below.
    if unsafe { libc::pipe2(bundle_fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        bail!(
            "pipe2() for exec bundle failed: {}",
            std::io::Error::last_os_error()
        );
    }
    // SAFETY: successful pipe2 returned uniquely owned descriptors.
    let bundle_reader = unsafe { OwnedFd::from_raw_fd(bundle_fds[0]) };
    // SAFETY: same ownership invariant for the writer.
    let bundle_writer = unsafe { OwnedFd::from_raw_fd(bundle_fds[1]) };
    let liveness = supervision::LivenessPipe::new()?;

    // All child-carried sources are above every fixed destination before fork,
    // so mapping fd 3, 20, and 21 cannot clobber a later source.
    let bundle_source = supervision::relocate_owned(bundle_reader.as_raw_fd(), "exec bundle")?;
    let liveness_source =
        supervision::relocate_owned(liveness.reader.as_raw_fd(), "parent liveness")?;
    let resource_safe =
        supervision::relocate_owned(resource_source.as_raw_fd(), "resource channel")?;
    drop(bundle_reader);
    drop(resource_source);
    drop(liveness.reader);

    let exec_resource = ExecResourceBinding {
        resource_id: resource_binding.resource_id.clone(),
        resource_name: resource_binding.resource_name.clone(),
        access: resource_binding.access,
        env_projection: read.env_projection,
        libsecret: resource_binding.libsecret,
        files_target_paths: resource_binding.files_target_paths.clone(),
    };
    let libsecret_in_scope = exec_resource.libsecret;
    let idmap_required = !read.access_paths.is_empty();
    let target = read.target;

    let inherited_sigchld = supervision::SigchldState::capture()?;
    inherited_sigchld.normalize_for_wait()?;

    // SAFETY: underwrap-exec is single-threaded here. Both branches restrict
    // post-fork work to owned-fd syscalls and the existing exec path.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        let error = std::io::Error::last_os_error();
        inherited_sigchld.restore()?;
        bail!("fork failed: {error}");
    }

    if pid == 0 {
        // The namespace stage must never own either parent-only writer.
        drop(bundle_writer);
        drop(liveness.writer);

        let child_result = (|| -> Result<()> {
            supervision::map_fixed(&bundle_source, EXEC_BUNDLE_FD, "exec bundle")?;
            supervision::map_fixed(
                &resource_safe,
                WRAPPER_RESOURCE_CHANNEL_FD,
                "wrapper-resource channel",
            )?;
            supervision::map_fixed(
                &liveness_source,
                WRAPPER_PARENT_LIVENESS_FD,
                "wrapper-parent liveness",
            )?;
            drop(bundle_source);
            drop(resource_safe);
            drop(liveness_source);
            inherited_sigchld.restore()?;

            use std::os::unix::process::CommandExt;
            let mut cmd = std::process::Command::new(ns_binary);
            cmd.arg(&target);
            cmd.env_remove("UNDERWRAP_LIBSECRET");
            cmd.env_remove("UNDERWRAP_DBUS_DAEMON");
            cmd.env_remove("UNDERWRAP_SECRETSD");
            cmd.env_remove("UNDERWRAP_ISOLATION_INSTANCE");
            cmd.env_remove("UNDERWRAP_ISOLATION_WRAPPER");
            cmd.env_remove(IDMAP_REQUIRED_ENV);
            cmd.env("UNDERWRAP_ISOLATION_INSTANCE", instance);
            cmd.env("UNDERWRAP_ISOLATION_WRAPPER", wrapper);
            if idmap_required {
                cmd.env(IDMAP_REQUIRED_ENV, "1");
            }
            if libsecret_in_scope {
                cmd.env("UNDERWRAP_LIBSECRET", "1");
                if let Some(ref bin) = config.dbus_daemon_binary {
                    cmd.env("UNDERWRAP_DBUS_DAEMON", bin);
                }
                if let Some(ref bin) = config.secretsd_binary {
                    cmd.env("UNDERWRAP_SECRETSD", bin);
                }
            }
            cmd.args(args);
            Err(anyhow::anyhow!("exec underwrap-ns failed: {}", cmd.exec()))
        })();
        if let Err(error) = child_result {
            eprintln!("underwrap-exec: {error:#}");
        }
        // SAFETY: this is the post-fork child error path; _exit avoids running
        // duplicated parent-owned destructors.
        unsafe { libc::_exit(1) }
    }

    // Parent owns the sole outer-liveness writer until underwrap-ns is reaped.
    drop(bundle_source);
    drop(resource_safe);
    drop(liveness_source);

    let payload = ExecBundle {
        env_passthrough: read.env_passthrough,
        env_set: read.env_set,
        target_runtime_dir_base: read.target_runtime_dir_base,
        resource: exec_resource,
        access_paths: read.access_paths,
    };
    let write_result = {
        let mut pipe = std::fs::File::from(bundle_writer);
        write_frame_sync(&mut pipe, &payload).context("writing exec bundle to pipe")
    };
    if let Err(error) = write_result {
        // SAFETY: pid is our unreaped child. SIGKILL bounds an error path; the
        // EINTR-tolerant wait below consumes it before returning.
        supervision::kill_and_reap(pid);
        return Err(error);
    }

    let status = match supervision::wait_terminal(pid) {
        Ok(status) => status,
        Err(error) => {
            supervision::kill_and_reap(pid);
            return Err(error);
        }
    };
    drop(liveness.writer);
    supervision::propagate_wait_status(status)
}

fn drop_privilege() -> Result<()> {
    let ruid = unsafe { libc::getuid() };
    let rgid = unsafe { libc::getgid() };

    let ret = unsafe { libc::setgroups(0, std::ptr::null()) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::EPERM) {
            bail!("setgroups(0) failed: {err}");
        }
    }

    // Permanently drop any elevated group identity, including the saved set-GID,
    // so the wrapper cannot later regain the setgid-derived identity.
    let ret = unsafe { libc::setresgid(rgid, rgid, rgid) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        bail!("setresgid({rgid}, {rgid}, {rgid}) failed: {err}");
    }

    // Permanently drop any elevated user identity, including the saved set-UID,
    // so there is no regain path through saved credentials.
    let ret = unsafe { libc::setresuid(ruid, ruid, ruid) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        bail!("setresuid({ruid}, {ruid}, {ruid}) failed: {err}");
    }

    Ok(())
}

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
