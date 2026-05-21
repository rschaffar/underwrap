//! underwrap-exec — the wrapper binary (stage 1).
//!
//! Installed via NixOS security.wrappers with setgid for GID-based identity.
//!
//! 1. Verifies wrapper identity from effective GID (not argv[0])
//! 2. Loads sidecar config by trusted executable basename
//! 3. Connects to the wrapper socket, gets env bindings and one
//!    wrapper-resource channel fd per resource via SCM_RIGHTS
//! 4. Drops all elevated privileges
//! 5. Forks: parent writes ExecBundle to fd 3 pipe, child execs underwrap-ns
//! 6. underwrap-ns drives Checkout on each channel, sets up libsecret/files
//!    staging, then creates a fresh PID namespace and execs the target
//!
//! Secret material flows through the pipe on fd 3 — never argv, env,
//! or filesystem between the two stages.

use anyhow::{Context, Result, bail};
use std::os::unix::net::UnixStream;

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::exec::{ExecBundle, ExecResourceBinding};
use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{SecretBytes, WrapperRequest, WrapperResponse};

/// Fixed config directory used when running with elevated privileges.
const TRUSTED_CONFIG_DIR: &str = "/etc/underwrap/wrappers";

/// Path to the underwrap-ns binary. Set by the NixOS module via sidecar config.
const DEFAULT_NS_BINARY: &str = "/run/wrappers/bin/underwrap-ns";

/// Fd used to pass the wrapper-resource channel into underwrap-ns.
/// The exec bundle pipe takes fd 3; the channel is mapped to fd 20.
const RESOURCE_CHANNEL_FD: i32 = 20;

struct ReceivedResourceBinding {
    resource_id: String,
    resource_name: String,
    access: AccessMode,
    libsecret: bool,
    files_target_paths: Vec<String>,
    channel_fd: i32,
}

type ReadSecretResult = (
    std::collections::HashMap<String, SecretBytes>,
    ReceivedResourceBinding,
);

fn main() -> Result<()> {
    // Wrapper identity still comes exclusively from the kernel-assigned
    // effective GID, never from argv[0]. The executable basename is used
    // only to select the trusted sidecar config, which allows distinct
    // root-managed wrapper filenames (for example unsafe debug variants)
    // to share the same wrapper GID and daemon-facing identity.
    ensure_underwrap_group_from_egid()?;
    let config_name = detect_wrapper_config_name_from_executable()?;
    let all_args: Vec<String> = std::env::args().skip(1).collect();

    let wrapper_config = load_wrapper_config(&config_name)?;

    let (env_bindings, resource_binding) = request_secret(&wrapper_config.wrapper_socket)?;

    // Drop all elevated privileges.
    drop_privilege()?;

    let ns_binary = wrapper_config
        .ns_binary
        .as_deref()
        .unwrap_or(DEFAULT_NS_BINARY);
    if !ns_binary.starts_with('/') {
        bail!("ns_binary must be an absolute path, got: {ns_binary:?}");
    }

    exec_via_ns(
        ns_binary,
        &wrapper_config,
        &all_args,
        env_bindings,
        resource_binding,
    )
}

/// Fork, pipe ExecBundle to the child, child execs underwrap-ns.
fn exec_via_ns(
    ns_binary: &str,
    config: &WrapperExecConfig,
    args: &[String],
    env_bindings: std::collections::HashMap<String, SecretBytes>,
    resource_binding: ReceivedResourceBinding,
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

    let exec_resource = ExecResourceBinding {
        resource_id: resource_binding.resource_id.clone(),
        resource_name: resource_binding.resource_name.clone(),
        access: resource_binding.access,
        channel_fd: RESOURCE_CHANNEL_FD as u32,
        libsecret: resource_binding.libsecret,
        files_target_paths: resource_binding.files_target_paths.clone(),
    };

    let libsecret_in_scope = exec_resource.libsecret;

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

        // Build argv for underwrap-ns: <ns_binary> <target> [args...]
        use std::os::unix::process::CommandExt;
        let mut cmd = std::process::Command::new(ns_binary);
        cmd.arg(&config.target);

        // ALWAYS strip internal env vars from the child — a malicious user
        // could set these to inject code into the PID namespace.
        cmd.env_remove("UNDERWRAP_LIBSECRET");
        cmd.env_remove("UNDERWRAP_DBUS_DAEMON");
        cmd.env_remove("UNDERWRAP_SECRETSD");
        cmd.env_remove("UNDERWRAP_DEBUG_INSTANCE");
        cmd.env_remove("UNDERWRAP_DEBUG_LOGICAL_WRAPPER");
        cmd.env_remove("UNDERWRAP_DEBUG_WRAPPER");
        cmd.env_remove("UNDERWRAP_DEBUG_ORIGINAL_TARGET");
        cmd.env_remove("UNDERWRAP_DEBUG_SECRETSD_TRACE_METADATA");
        if libsecret_in_scope {
            cmd.env("UNDERWRAP_LIBSECRET", "1");
            if let Some(ref bin) = config.dbus_daemon_binary {
                cmd.env("UNDERWRAP_DBUS_DAEMON", bin);
            }
            if let Some(ref bin) = config.secretsd_binary {
                cmd.env("UNDERWRAP_SECRETSD", bin);
            }
        }

        for key in [
            "UNDERWRAP_DEBUG_INSTANCE",
            "UNDERWRAP_DEBUG_LOGICAL_WRAPPER",
            "UNDERWRAP_DEBUG_WRAPPER",
            "UNDERWRAP_DEBUG_ORIGINAL_TARGET",
            "UNDERWRAP_DEBUG_SECRETSD_TRACE_METADATA",
        ] {
            if let Some(value) = config.env_set.get(key) {
                cmd.env(key, value);
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
            env: env_bindings,
            env_passthrough: config.env_passthrough.clone(),
            env_set: config.env_set.clone(),
            resource: exec_resource,
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

// ---------------------------------------------------------------------------

/// The prefix used by the NixOS module for wrapper group names.
const GROUP_PREFIX: &str = "underwrap-";

/// Verify that the kernel-assigned effective GID belongs to an underwrap
/// wrapper group. This is the ONLY trusted source of wrapper identity.
fn ensure_underwrap_group_from_egid() -> Result<()> {
    let egid = unsafe { libc::getegid() };
    let group_name = resolve_gid_name(egid)
        .with_context(|| format!("cannot resolve effective GID {egid} to a group name"))?;

    group_name.strip_prefix(GROUP_PREFIX).with_context(|| {
        format!(
            "effective GID group '{group_name}' does not start with '{GROUP_PREFIX}' — \
             not an underwrap wrapper group"
        )
    })?;

    Ok(())
}

/// Derive the sidecar config name from the actual executed wrapper binary.
/// `/proc/self/exe` is kernel-provided and refers to the real file that was
/// executed, unlike argv[0] which is user-controlled.
fn detect_wrapper_config_name_from_executable() -> Result<String> {
    let exe_path = std::fs::read_link("/proc/self/exe")
        .context("reading /proc/self/exe for wrapper config")?;
    config_name_from_exe_path(&exe_path)
}

fn config_name_from_exe_path(path: &std::path::Path) -> Result<String> {
    let file_name = path.file_name().with_context(|| {
        format!(
            "wrapper executable path '{}' has no final path component",
            path.display()
        )
    })?;
    let file_name = file_name
        .to_str()
        .context("wrapper executable name is not valid UTF-8")?;
    if file_name.is_empty() {
        bail!("wrapper executable name must not be empty");
    }
    Ok(file_name.to_string())
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

struct WrapperExecConfig {
    target: String,
    wrapper_socket: String,
    ns_binary: Option<String>,
    /// Path to the dbus-daemon binary (for libsecret mode).
    dbus_daemon_binary: Option<String>,
    /// Path to the underwrap-secretsd binary (for libsecret mode).
    secretsd_binary: Option<String>,
    /// Env vars to pass through from the user's environment to the target.
    env_passthrough: Vec<String>,
    /// Fixed env vars to set on the target (from trusted config).
    env_set: std::collections::HashMap<String, String>,
}

fn config_dir() -> String {
    // Always use the trusted config directory. No environment variable
    // override — that would allow config redirection attacks when the
    // binary is run without the setgid wrapper.
    TRUSTED_CONFIG_DIR.to_string()
}

fn load_wrapper_config(config_name: &str) -> Result<WrapperExecConfig> {
    let dir = config_dir();
    let path = format!("{dir}/{config_name}.yaml");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading wrapper config {path}"))?;

    #[derive(serde::Deserialize)]
    struct WrapperFile {
        target: String,
        wrapper_socket: String,
        #[serde(default)]
        ns_binary: Option<String>,
        // libsecret flag is left in the sidecar schema for backward
        // compatibility with the existing NixOS module output, but the
        // wrapper now determines libsecret-in-scope from the daemon's
        // resource list, not from this flag.
        #[serde(default)]
        #[allow(dead_code)]
        libsecret: bool,
        #[serde(default)]
        dbus_daemon_binary: Option<String>,
        #[serde(default)]
        secretsd_binary: Option<String>,
        #[serde(default)]
        env_passthrough: Vec<String>,
        #[serde(default)]
        env_set: std::collections::HashMap<String, String>,
    }

    let wf: WrapperFile = serde_yaml::from_str(&text)?;
    if !wf.target.starts_with('/') {
        bail!(
            "wrapper target must be an absolute path, got: {:?}",
            wf.target
        );
    }

    Ok(WrapperExecConfig {
        target: wf.target,
        wrapper_socket: wf.wrapper_socket,
        ns_binary: wf.ns_binary,
        dbus_daemon_binary: wf.dbus_daemon_binary,
        secretsd_binary: wf.secretsd_binary,
        env_passthrough: wf.env_passthrough,
        env_set: wf.env_set,
    })
}

/// Request credentials from the daemon. Returns env bindings plus the
/// single wrapper-resource channel fd the daemon authorized.
fn request_secret(socket_path: &str) -> Result<ReadSecretResult> {
    let mut stream = UnixStream::connect(socket_path)
        .with_context(|| format!("connecting to daemon at {socket_path}"))?;

    let scope = std::env::var("UNDERWRAP_SCOPE").ok();
    let req = WrapperRequest::Read { scope };
    write_frame_sync(&mut stream, &req)?;

    let response: WrapperResponse = read_frame_sync(&mut stream)?;

    let (env_bindings, resource_id, resource_name, access, libsecret, files_target_paths) =
        match response {
            WrapperResponse::ReadOk {
                env,
                resource_id,
                resource_name,
                access,
                libsecret,
                files_target_paths,
            } => (
                env,
                resource_id,
                resource_name,
                access,
                libsecret,
                files_target_paths,
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

    Ok((
        env_bindings,
        ReceivedResourceBinding {
            resource_id,
            resource_name,
            access,
            libsecret,
            files_target_paths,
            channel_fd: fd,
        },
    ))
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
mod tests {
    use super::config_name_from_exe_path;

    #[test]
    fn config_name_uses_executable_basename() {
        let name = config_name_from_exe_path(std::path::Path::new(
            "/run/wrappers/bin/debug-robert-gh-authorized",
        ))
        .unwrap();
        assert_eq!(name, "debug-robert-gh-authorized");
    }

    #[test]
    fn config_name_rejects_missing_basename() {
        let err = config_name_from_exe_path(std::path::Path::new("/"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("no final path component"));
    }
}
