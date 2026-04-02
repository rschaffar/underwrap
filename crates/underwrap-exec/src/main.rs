//! underwrap-exec — the minimal wrapper binary.
//!
//! Designed to be installed under wrapper-specific names (symlinks or copies).
//! Determines which wrapper it is from argv[0], contacts the daemon, receives
//! secret material, injects it, drops privilege, and execs the target.
//!
//! Security hardening:
//! - Ignores UNDERWRAP_CONFIG_DIR under elevated privilege
//! - Validates target is an absolute path
//! - Drops UID, GID, supplementary groups before exec
//! - Sets PR_SET_DUMPABLE=0 via pre_exec hook so /proc/<pid>/environ
//!   is owned by root:root, preventing same-UID processes from reading
//!   the injected secret from the child's environment

use anyhow::{Context, Result, bail};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;

use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{ReadRequest, ReadResponse};

/// Fixed config directory used when running with elevated privileges.
/// UNDERWRAP_CONFIG_DIR is ignored in that case to prevent secret exfil.
const TRUSTED_CONFIG_DIR: &str = "/etc/underwrap/wrappers";

fn main() -> Result<()> {
    let wrapper_name = detect_wrapper_name()?;
    let args: Vec<String> = std::env::args().skip(1).collect();

    let wrapper_config = load_wrapper_config(&wrapper_name)?;

    // Request secret from daemon.
    let material = request_secret(&wrapper_name, &wrapper_config.read_socket)?;

    // Drop supplementary groups / setgid / setuid before exec.
    drop_privilege()?;

    // Build child command.
    let mut cmd = std::process::Command::new(&wrapper_config.target);
    cmd.args(&args);
    cmd.env(&wrapper_config.env_variable, &material);

    // Install pre_exec hook to set PR_SET_DUMPABLE=0.
    //
    // This runs in the child after fork() but before exec(). It causes
    // the kernel to set /proc/<pid>/* ownership to root:root, which:
    // - Blocks same-UID processes from reading /proc/<pid>/environ
    // - Blocks ptrace attach from same-UID processes (even with ptrace_scope=0)
    // - Prevents core dumps from leaking the secret
    //
    // The child process itself could undo this with prctl(PR_SET_DUMPABLE, 1),
    // but if the child is compromised it already has the secret in memory.
    //
    // SAFETY: prctl(PR_SET_DUMPABLE, 0) is async-signal-safe and cannot fail
    // with these arguments. The closure only calls this one syscall.
    unsafe {
        cmd.pre_exec(|| {
            let ret = libc::prctl(libc::PR_SET_DUMPABLE, 0);
            if ret != 0 {
                // Should never happen, but don't proceed if it does.
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let err = cmd.exec();
    bail!("exec failed: {err}");
}

/// Determine wrapper name from argv[0].
fn detect_wrapper_name() -> Result<String> {
    let argv0 = std::env::args().next().context("missing argv[0]")?;
    let name = std::path::Path::new(&argv0)
        .file_name()
        .context("no file name in argv[0]")?
        .to_str()
        .context("non-UTF-8 argv[0]")?
        .to_string();
    Ok(name)
}

/// Per-wrapper config loaded from a sidecar YAML.
struct WrapperExecConfig {
    target: String,
    env_variable: String,
    read_socket: String,
}

/// Determine the config directory.
///
/// **Security**: when running with elevated privileges (setuid/setgid),
/// the UNDERWRAP_CONFIG_DIR env var is ignored and we use a fixed
/// trusted path. This prevents a caller from redirecting the wrapper
/// to an attacker-controlled config that exfiltrates secrets.
fn config_dir() -> String {
    if is_elevated() {
        return TRUSTED_CONFIG_DIR.to_string();
    }
    std::env::var("UNDERWRAP_CONFIG_DIR").unwrap_or_else(|_| TRUSTED_CONFIG_DIR.to_string())
}

/// Check if we're running with elevated privileges (setuid or setgid).
fn is_elevated() -> bool {
    unsafe { libc::geteuid() != libc::getuid() || libc::getegid() != libc::getgid() }
}

fn load_wrapper_config(wrapper_name: &str) -> Result<WrapperExecConfig> {
    let dir = config_dir();
    let path = format!("{dir}/{wrapper_name}.yaml");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading wrapper config {path}"))?;

    #[derive(serde::Deserialize)]
    struct WrapperFile {
        target: String,
        #[serde(default = "default_env")]
        env_variable: String,
        #[serde(default = "default_socket")]
        read_socket: String,
    }
    fn default_env() -> String {
        "UNDERWRAP_SECRET".to_string()
    }
    fn default_socket() -> String {
        underwrap_protocol::READ_SOCKET_PATH.to_string()
    }

    let wf: WrapperFile = serde_yaml::from_str(&text)?;

    // Validate target is an absolute path.
    if !wf.target.starts_with('/') {
        bail!(
            "wrapper target must be an absolute path, got: {:?}",
            wf.target
        );
    }

    Ok(WrapperExecConfig {
        target: wf.target,
        env_variable: wf.env_variable,
        read_socket: wf.read_socket,
    })
}

fn request_secret(wrapper_name: &str, socket_path: &str) -> Result<String> {
    let mut stream = UnixStream::connect(socket_path)
        .with_context(|| format!("connecting to daemon at {socket_path}"))?;

    let scope = std::env::var("UNDERWRAP_SCOPE").ok();
    let req = ReadRequest {
        wrapper: wrapper_name.to_string(),
        scope,
    };
    write_frame_sync(&mut stream, &req)?;

    let response: ReadResponse = read_frame_sync(&mut stream)?;
    match response {
        ReadResponse::Ok { material } => Ok(material),
        ReadResponse::Error { code, message } => {
            bail!("daemon denied request ({code:?}): {message}");
        }
    }
}

/// Drop all elevated privileges (UID, GID, supplementary groups) before exec.
///
/// Aborts the process if privilege cannot be dropped — executing the target
/// with elevated privileges is worse than failing.
fn drop_privilege() -> Result<()> {
    let ruid = unsafe { libc::getuid() };
    let rgid = unsafe { libc::getgid() };

    // Drop supplementary groups first (requires privilege).
    let ret = unsafe { libc::setgroups(0, std::ptr::null()) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        // EPERM is expected for unprivileged users — no groups to drop.
        if err.raw_os_error() != Some(libc::EPERM) {
            bail!("setgroups(0) failed: {err}");
        }
    }

    // Drop GID.
    if unsafe { libc::getegid() } != rgid {
        let ret = unsafe { libc::setgid(rgid) };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            bail!("setgid({rgid}) failed: {err}");
        }
    }

    // Drop UID (must be last — after this we lose privilege to change anything).
    if unsafe { libc::geteuid() } != ruid {
        let ret = unsafe { libc::setuid(ruid) };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            bail!("setuid({ruid}) failed: {err}");
        }
    }

    Ok(())
}
