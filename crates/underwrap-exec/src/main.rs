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
//!
//! Known limitation: after privilege drop + exec of a non-setuid target,
//! the child process is dumpable, meaning same-UID processes can read
//! /proc/<pid>/environ. PID namespace isolation (CLONE_NEWPID) would
//! prevent this but is incompatible with NixOS security.wrappers' setgid
//! model. See TODO.md for details.

use anyhow::{Context, Result, bail};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;

use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{ReadRequest, ReadResponse};

/// Fixed config directory used when running with elevated privileges.
const TRUSTED_CONFIG_DIR: &str = "/etc/underwrap/wrappers";

fn main() -> Result<()> {
    let wrapper_name = detect_wrapper_name()?;
    let args: Vec<String> = std::env::args().skip(1).collect();

    let wrapper_config = load_wrapper_config(&wrapper_name)?;

    // Request secret from daemon (while still setgid — daemon checks egid).
    let material = request_secret(&wrapper_name, &wrapper_config.read_socket)?;

    // Drop all elevated privileges.
    drop_privilege()?;

    // Exec the target with the secret in its environment.
    let mut cmd = std::process::Command::new(&wrapper_config.target);
    cmd.args(&args);
    cmd.env(&wrapper_config.env_variable, &material);

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

struct WrapperExecConfig {
    target: String,
    env_variable: String,
    read_socket: String,
}

fn config_dir() -> String {
    if is_elevated() {
        return TRUSTED_CONFIG_DIR.to_string();
    }
    std::env::var("UNDERWRAP_CONFIG_DIR").unwrap_or_else(|_| TRUSTED_CONFIG_DIR.to_string())
}

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

/// Drop all elevated privileges (UID, GID, supplementary groups).
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

    if unsafe { libc::getegid() } != rgid {
        let ret = unsafe { libc::setgid(rgid) };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            bail!("setgid({rgid}) failed: {err}");
        }
    }

    if unsafe { libc::geteuid() } != ruid {
        let ret = unsafe { libc::setuid(ruid) };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            bail!("setuid({ruid}) failed: {err}");
        }
    }

    Ok(())
}
