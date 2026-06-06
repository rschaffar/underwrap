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
//!    variant, scope }`; receives the env bindings, the daemon-chosen
//!    target binary, and one wrapper-resource channel fd via SCM_RIGHTS
//! 5. Drops all elevated privileges
//! 6. Forks: parent writes the `ExecBundle` to fd 3; child execs
//!    `underwrap-ns`, which sets up the namespace and execs the target
//!
//! The wrapper never chooses the target binary: it execs whatever path
//! the daemon returned in `ReadOk`. Secret material flows through the
//! pipe on fd 3 — never argv, env, or filesystem between the two stages.

use anyhow::{Context, Result, bail};
use std::os::unix::net::UnixStream;

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::exec::{ExecBundle, ExecResourceBinding};
use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{SecretBytes, WrapperRequest, WrapperResponse};
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

/// Fd used to pass the wrapper-resource channel into underwrap-ns.
/// The exec bundle pipe takes fd 3; the channel is mapped to fd 20.
const RESOURCE_CHANNEL_FD: i32 = 20;

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
    env: std::collections::HashMap<String, SecretBytes>,
    target: String,
    env_passthrough: Vec<String>,
    env_set: std::collections::HashMap<String, String>,
    xdg_runtime_dir_base: String,
    idmap_paths: Vec<String>,
    resource_binding: ReceivedResourceBinding,
}

struct ReceivedResourceBinding {
    resource_id: String,
    resource_name: String,
    access: AccessMode,
    libsecret: bool,
    files_target_paths: Vec<String>,
    channel_fd: i32,
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

    exec_via_ns(ns_binary, &instance_config, &invocation.target_args, read)
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
        env,
        resource_id,
        resource_name,
        access,
        libsecret,
        files_target_paths,
        target,
        env_passthrough,
        env_set,
        xdg_runtime_dir_base,
        idmap_paths,
    ) = match response {
        WrapperResponse::ReadOk {
            env,
            resource_id,
            resource_name,
            access,
            libsecret,
            files_target_paths,
            target,
            env_passthrough,
            env_set,
            xdg_runtime_dir_base,
            idmap_paths,
        } => (
            env,
            resource_id,
            resource_name,
            access,
            libsecret,
            files_target_paths,
            target,
            env_passthrough,
            env_set,
            xdg_runtime_dir_base,
            idmap_paths,
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

    let fd = underwrap_protocol::scm_rights::recv_fd(&stream).with_context(|| {
        format!("failed to receive wrapper-resource channel fd for '{resource_id}'")
    })?;

    Ok(ReadResult {
        env,
        target: target.to_string_lossy().into_owned(),
        env_passthrough,
        env_set,
        xdg_runtime_dir_base,
        idmap_paths,
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

/// Fork, pipe ExecBundle to the child, child execs underwrap-ns.
fn exec_via_ns(
    ns_binary: &str,
    config: &InstanceConfig,
    args: &[String],
    read: ReadResult,
) -> Result<()> {
    if !std::path::Path::new(ns_binary).exists() {
        bail!(
            "underwrap-ns not found at {ns_binary}. \
             PID namespace isolation requires underwrap-ns with cap_sys_admin."
        );
    }

    // Make this process non-dumpable. Since we don't exec after this point
    // (we fork and waitpid), the flag persists. This prevents same-UID
    // processes from reading the secret out of our /proc/<pid>/mem.
    let ret = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) };
    if ret != 0 {
        bail!(
            "prctl(PR_SET_DUMPABLE, 0) failed: {}",
            std::io::Error::last_os_error()
        );
    }

    // Create pipe for the ExecBundle transfer.
    let mut pipe_fds = [0i32; 2];
    let ret = unsafe { libc::pipe(pipe_fds.as_mut_ptr()) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        bail!("pipe() failed: {err}");
    }
    let (pipe_read, pipe_write) = (pipe_fds[0], pipe_fds[1]);

    let resource_binding = read.resource_binding;
    let exec_resource = ExecResourceBinding {
        resource_id: resource_binding.resource_id.clone(),
        resource_name: resource_binding.resource_name.clone(),
        access: resource_binding.access,
        channel_fd: RESOURCE_CHANNEL_FD as u32,
        libsecret: resource_binding.libsecret,
        files_target_paths: resource_binding.files_target_paths.clone(),
    };

    let libsecret_in_scope = exec_resource.libsecret;
    let target = read.target;

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        let err = std::io::Error::last_os_error();
        bail!("fork failed: {err}");
    }

    if pid == 0 {
        // === Child: exec underwrap-ns ===

        // Close write end of pipe.
        unsafe { libc::close(pipe_write) };

        // Move read end to fd 3 (the expected fd for underwrap-ns).
        if pipe_read != 3 {
            let ret = unsafe { libc::dup2(pipe_read, 3) };
            if ret < 0 {
                let err = std::io::Error::last_os_error();
                eprintln!("underwrap-exec: dup2 failed: {err}");
                std::process::exit(1);
            }
            unsafe { libc::close(pipe_read) };
        }

        // Map the wrapper-resource fd to its agreed slot.
        let target_fd = exec_resource.channel_fd as i32;
        if resource_binding.channel_fd != target_fd {
            let ret = unsafe { libc::dup2(resource_binding.channel_fd, target_fd) };
            if ret < 0 {
                let err = std::io::Error::last_os_error();
                eprintln!("underwrap-exec: dup2 wrapper-resource channel fd failed: {err}");
                std::process::exit(1);
            }
            unsafe { libc::close(resource_binding.channel_fd) };
        }

        // Build argv for underwrap-ns: <ns_binary> <target> [args...].
        // The target is the daemon-chosen path, never a local choice.
        use std::os::unix::process::CommandExt;
        let mut cmd = std::process::Command::new(ns_binary);
        cmd.arg(&target);

        // ALWAYS strip internal env vars from the child — a malicious user
        // could set these to inject code into the PID namespace.
        cmd.env_remove("UNDERWRAP_LIBSECRET");
        cmd.env_remove("UNDERWRAP_DBUS_DAEMON");
        cmd.env_remove("UNDERWRAP_SECRETSD");
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

        let err = cmd.exec();
        eprintln!("underwrap-exec: exec underwrap-ns failed: {err}");
        std::process::exit(1);
    }

    // === Parent: write ExecBundle to pipe, wait for child ===

    unsafe { libc::close(pipe_read) };

    unsafe { libc::close(resource_binding.channel_fd) };

    {
        use std::os::unix::io::FromRawFd;
        let mut pipe = unsafe { std::fs::File::from_raw_fd(pipe_write) };
        let payload = ExecBundle {
            env: read.env,
            env_passthrough: read.env_passthrough,
            env_set: read.env_set,
            xdg_runtime_dir_base: read.xdg_runtime_dir_base,
            resource: exec_resource,
            idmap_paths: read.idmap_paths,
        };
        write_frame_sync(&mut pipe, &payload).context("writing exec bundle to pipe")?;
        // pipe_write closed on drop.
    }

    let mut status: libc::c_int = 0;
    let ret = unsafe { libc::waitpid(pid, &mut status, 0) };
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        bail!("waitpid failed: {err}");
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
