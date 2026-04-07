//! underwrap-exec — the wrapper binary (stage 1).
//!
//! Installed via NixOS security.wrappers with setgid for GID-based identity.
//!
//! 1. Detects wrapper name from argv[0]
//! 2. Loads sidecar config from trusted path
//! 3. Connects to daemon, gets secret (daemon verifies egid)
//! 4. Drops all elevated privileges
//! 5. Forks: parent writes secret to pipe, child execs underwrap-ns
//! 6. underwrap-ns creates PID namespace and execs the target
//!
//! The secret passes through a pipe fd — never in env, argv, or filesystem
//! between the two stages.

use anyhow::{Context, Result, bail};
use std::os::unix::net::UnixStream;

use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{ReadRequest, ReadResponse};

/// Fixed config directory used when running with elevated privileges.
const TRUSTED_CONFIG_DIR: &str = "/etc/underwrap/wrappers";

/// Path to the underwrap-ns binary. Set by the NixOS module via sidecar config.
/// Falls back to searching next to the current executable.
const DEFAULT_NS_BINARY: &str = "/run/wrappers/bin/underwrap-ns";

fn main() -> Result<()> {
    let wrapper_name = detect_wrapper_name()?;
    let args: Vec<String> = std::env::args().skip(1).collect();

    let wrapper_config = load_wrapper_config(&wrapper_name)?;

    // Request secret from daemon (while still setgid — daemon checks egid).
    let mut material = request_secret(&wrapper_name, &wrapper_config.read_socket)?;

    // Drop all elevated privileges.
    drop_privilege()?;

    let ns_binary = wrapper_config
        .ns_binary
        .as_deref()
        .unwrap_or(DEFAULT_NS_BINARY);

    exec_via_ns(ns_binary, &wrapper_config, &args, &mut material)
}

/// Fork, pipe the secret to the child, child execs underwrap-ns.
fn exec_via_ns(
    ns_binary: &str,
    config: &WrapperExecConfig,
    args: &[String],
    material: &mut String,
) -> Result<()> {
    // Make this process non-dumpable. Since we don't exec after this point
    // (we fork and waitpid), the flag persists. This prevents same-UID
    // processes from reading the secret out of our /proc/<pid>/mem.
    unsafe {
        libc::prctl(libc::PR_SET_DUMPABLE, 0);
    }

    // Create pipe for secret transfer.
    let mut pipe_fds = [0i32; 2];
    let ret = unsafe { libc::pipe(pipe_fds.as_mut_ptr()) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        bail!("pipe() failed: {err}");
    }
    let (pipe_read, pipe_write) = (pipe_fds[0], pipe_fds[1]);

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

        // Build argv for underwrap-ns: <ns_binary> <env_var> <target> [args...]
        use std::os::unix::process::CommandExt;
        let mut cmd = std::process::Command::new(ns_binary);
        cmd.arg(&config.env_variable);
        cmd.arg(&config.target);
        cmd.args(args);

        let err = cmd.exec();
        eprintln!("underwrap-exec: exec underwrap-ns failed: {err}");
        std::process::exit(1);
    }

    // === Parent: write secret to pipe, wait for child ===

    // Close read end.
    unsafe { libc::close(pipe_read) };

    // Write secret to pipe.
    {
        use std::io::Write;
        use std::os::unix::io::FromRawFd;
        let mut pipe = unsafe { std::fs::File::from_raw_fd(pipe_write) };
        pipe.write_all(material.as_bytes())
            .context("writing secret to pipe")?;
        // pipe_write closed on drop.
    }

    // Zeroize the secret from this process's memory.
    use zeroize::Zeroize;
    material.zeroize();

    // Wait for child.
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
    ns_binary: Option<String>,
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
        #[serde(default)]
        ns_binary: Option<String>,
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
        ns_binary: wf.ns_binary,
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
