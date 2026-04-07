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
use underwrap_protocol::message::{WrapperRequest, WrapperResponse};

/// Fixed config directory used when running with elevated privileges.
const TRUSTED_CONFIG_DIR: &str = "/etc/underwrap/wrappers";

/// Path to the underwrap-ns binary. Set by the NixOS module via sidecar config.
/// Falls back to searching next to the current executable.
const DEFAULT_NS_BINARY: &str = "/run/wrappers/bin/underwrap-ns";

fn main() -> Result<()> {
    let wrapper_name = detect_wrapper_name()?;
    let all_args: Vec<String> = std::env::args().skip(1).collect();

    // Check if this is an unlock request.
    if all_args.first().map(|s| s.as_str()) == Some("--unlock") {
        return handle_unlock(&wrapper_name, &all_args[1..]);
    }

    let wrapper_config = load_wrapper_config(&wrapper_name)?;

    // Request secret from daemon (while still setgid — daemon checks egid).
    // If libsecret mode, also receive the secretsd channel fd.
    let (mut material, channel_fd) = request_secret(
        &wrapper_name,
        &wrapper_config.read_socket,
        wrapper_config.libsecret,
    )?;

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
        &mut material,
        channel_fd,
    )
}

/// Handle --unlock: request unlock via the read socket with SSH agent challenge.
fn handle_unlock(wrapper_name: &str, args: &[String]) -> Result<()> {
    let wrapper_config = load_wrapper_config(wrapper_name)?;

    // Parse --seconds N and --scope S from args.
    let mut seconds: Option<u64> = None;
    let mut scope: Option<String> = std::env::var("UNDERWRAP_SCOPE").ok();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--seconds" => {
                i += 1;
                seconds = Some(
                    args.get(i)
                        .context("--seconds requires a value")?
                        .parse()
                        .context("--seconds must be a number")?,
                );
            }
            "--scope" => {
                i += 1;
                scope = Some(args.get(i).context("--scope requires a value")?.clone());
            }
            other => bail!("unknown unlock argument: {other}"),
        }
        i += 1;
    }

    // Step 1: Send unlock request.
    let mut stream = UnixStream::connect(&wrapper_config.read_socket)
        .with_context(|| format!("connecting to daemon at {}", wrapper_config.read_socket))?;

    let req = WrapperRequest::Unlock {
        wrapper: wrapper_name.to_string(),
        scope: scope.clone(),
        seconds,
    };
    write_frame_sync(&mut stream, &req)?;
    let resp: WrapperResponse = read_frame_sync(&mut stream)?;

    match resp {
        WrapperResponse::Ok { message, .. } => {
            // Unlocked without challenge (no SSH key required).
            drop(stream);
            eprintln!("unlocked: {}", message.unwrap_or_default());
            drop_privilege()?;
            Ok(())
        }
        WrapperResponse::Challenge { nonce } => {
            // Need to sign the nonce with SSH agent.
            // Sign BEFORE dropping privilege — SSH agent access is fine.
            // Send response on the SAME connection — reconnecting would
            // lose the wrapper's effective GID after drop_privilege().
            let (signature_b64, public_key_str) = sign_with_ssh_agent(&nonce)?;

            let req2 = WrapperRequest::UnlockResponse {
                wrapper: wrapper_name.to_string(),
                scope,
                seconds,
                challenge: nonce,
                signature: signature_b64,
                public_key: public_key_str,
            };
            write_frame_sync(&mut stream, &req2)?;
            let resp2: WrapperResponse = read_frame_sync(&mut stream)?;
            drop(stream);

            // Now drop privilege (no longer needed).
            drop_privilege()?;

            match resp2 {
                WrapperResponse::Ok { message, .. } => {
                    eprintln!("unlocked: {}", message.unwrap_or_default());
                    Ok(())
                }
                WrapperResponse::Error { message, .. } => {
                    bail!("unlock failed: {message}");
                }
                _ => bail!("unexpected response"),
            }
        }
        WrapperResponse::Error { message, .. } => {
            bail!("unlock request failed: {message}");
        }
    }
}

/// Sign a challenge nonce using the SSH agent (SSH_AUTH_SOCK).
fn sign_with_ssh_agent(nonce_hex: &str) -> Result<(String, String)> {
    let auth_sock = std::env::var("SSH_AUTH_SOCK").context("SSH_AUTH_SOCK not set")?;
    let mut agent = UnixStream::connect(&auth_sock)
        .with_context(|| format!("connecting to SSH agent at {auth_sock}"))?;

    let nonce_bytes = hex::decode(nonce_hex).context("invalid challenge hex")?;

    // Request identities from the agent.
    let identities = ssh_agent_request_identities(&mut agent)?;
    if identities.is_empty() {
        bail!("no keys in SSH agent");
    }

    // Try signing with the first key.
    let (key_blob, key_comment) = &identities[0];
    let signature = ssh_agent_sign(&mut agent, key_blob, &nonce_bytes)?;

    // Encode the public key in OpenSSH format.
    let public_key_str = format!(
        "{} {}",
        ssh_key_type_from_blob(key_blob),
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, key_blob)
    );

    let signature_b64 =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &signature);

    eprintln!("signed challenge with key: {key_comment}");
    Ok((signature_b64, public_key_str))
}

// ---------------------------------------------------------------------------
// Minimal SSH agent protocol implementation
// ---------------------------------------------------------------------------

const SSH_AGENTC_REQUEST_IDENTITIES: u8 = 11;
const SSH_AGENT_IDENTITIES_ANSWER: u8 = 12;
const SSH_AGENTC_SIGN_REQUEST: u8 = 13;
const SSH_AGENT_SIGN_RESPONSE: u8 = 14;

fn ssh_agent_request_identities(agent: &mut UnixStream) -> Result<Vec<(Vec<u8>, String)>> {
    use std::io::{Read, Write};

    // Send request.
    let msg = [SSH_AGENTC_REQUEST_IDENTITIES];
    let len = (msg.len() as u32).to_be_bytes();
    agent.write_all(&len)?;
    agent.write_all(&msg)?;

    // Read response.
    let mut len_buf = [0u8; 4];
    agent.read_exact(&mut len_buf)?;
    let resp_len = u32::from_be_bytes(len_buf) as usize;
    let mut resp = vec![0u8; resp_len];
    agent.read_exact(&mut resp)?;

    if resp[0] != SSH_AGENT_IDENTITIES_ANSWER {
        bail!("unexpected agent response type: {}", resp[0]);
    }

    let nkeys = u32::from_be_bytes([resp[1], resp[2], resp[3], resp[4]]) as usize;
    let mut pos = 5;
    let mut keys = Vec::new();

    for _ in 0..nkeys {
        if pos + 4 > resp.len() {
            break;
        }
        let blob_len =
            u32::from_be_bytes([resp[pos], resp[pos + 1], resp[pos + 2], resp[pos + 3]]) as usize;
        pos += 4;
        let blob = resp[pos..pos + blob_len].to_vec();
        pos += blob_len;

        let comment_len =
            u32::from_be_bytes([resp[pos], resp[pos + 1], resp[pos + 2], resp[pos + 3]]) as usize;
        pos += 4;
        let comment = String::from_utf8_lossy(&resp[pos..pos + comment_len]).to_string();
        pos += comment_len;

        keys.push((blob, comment));
    }

    Ok(keys)
}

fn ssh_agent_sign(agent: &mut UnixStream, key_blob: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    use std::io::{Read, Write};

    let mut msg = Vec::new();
    msg.push(SSH_AGENTC_SIGN_REQUEST);
    msg.extend_from_slice(&(key_blob.len() as u32).to_be_bytes());
    msg.extend_from_slice(key_blob);
    msg.extend_from_slice(&(data.len() as u32).to_be_bytes());
    msg.extend_from_slice(data);
    msg.extend_from_slice(&0u32.to_be_bytes()); // flags

    let len = (msg.len() as u32).to_be_bytes();
    agent.write_all(&len)?;
    agent.write_all(&msg)?;

    let mut len_buf = [0u8; 4];
    agent.read_exact(&mut len_buf)?;
    let resp_len = u32::from_be_bytes(len_buf) as usize;
    let mut resp = vec![0u8; resp_len];
    agent.read_exact(&mut resp)?;

    if resp[0] != SSH_AGENT_SIGN_RESPONSE {
        bail!("agent refused to sign (response type: {})", resp[0]);
    }

    // Parse signature blob.
    let sig_len = u32::from_be_bytes([resp[1], resp[2], resp[3], resp[4]]) as usize;
    let sig = resp[5..5 + sig_len].to_vec();

    Ok(sig)
}

fn ssh_key_type_from_blob(blob: &[u8]) -> &str {
    if blob.len() < 4 {
        return "ssh-unknown";
    }
    let type_len = u32::from_be_bytes([blob[0], blob[1], blob[2], blob[3]]) as usize;
    if blob.len() < 4 + type_len {
        return "ssh-unknown";
    }
    std::str::from_utf8(&blob[4..4 + type_len]).unwrap_or("ssh-unknown")
}

/// Fork, pipe the secret to the child, child execs underwrap-ns.
fn exec_via_ns(
    ns_binary: &str,
    config: &WrapperExecConfig,
    args: &[String],
    material: &mut String,
    channel_fd: Option<i32>,
) -> Result<()> {
    // Verify underwrap-ns exists before proceeding.
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

        // If we have a secretsd channel fd, move it to fd 4.
        if let Some(ch_fd) = channel_fd
            && ch_fd != 4
        {
            let ret = unsafe { libc::dup2(ch_fd, 4) };
            if ret < 0 {
                let err = std::io::Error::last_os_error();
                eprintln!("underwrap-exec: dup2 channel fd failed: {err}");
                std::process::exit(1);
            }
            unsafe { libc::close(ch_fd) };
        }

        // Build argv for underwrap-ns: <ns_binary> <env_var> <target> [args...]
        use std::os::unix::process::CommandExt;
        let mut cmd = std::process::Command::new(ns_binary);
        cmd.arg(&config.env_variable);
        cmd.arg(&config.target);

        // ALWAYS strip internal env vars from the child — a malicious user
        // could set these to inject code into the PID namespace.
        cmd.env_remove("UNDERWRAP_LIBSECRET");
        cmd.env_remove("UNDERWRAP_DBUS_DAEMON");
        cmd.env_remove("UNDERWRAP_SECRETSD");
        cmd.env_remove("UNDERWRAP_ENV_CONFIG");
        if config.libsecret {
            // channel_fd is guaranteed Some here — we bail above if recv_fd fails.
            cmd.env("UNDERWRAP_LIBSECRET", "1");
            if let Some(ref bin) = config.dbus_daemon_binary {
                cmd.env("UNDERWRAP_DBUS_DAEMON", bin);
            }
            if let Some(ref bin) = config.secretsd_binary {
                cmd.env("UNDERWRAP_SECRETSD", bin);
            }
        }

        // Pass env sanitization config from the trusted sidecar to underwrap-ns.
        // This is a JSON blob containing passthrough list and fixed vars.
        let env_config = serde_json::json!({
            "passthrough": config.env_passthrough,
            "set": config.env_set,
        });
        cmd.env("UNDERWRAP_ENV_CONFIG", env_config.to_string());

        cmd.args(args);

        let err = cmd.exec();
        eprintln!("underwrap-exec: exec underwrap-ns failed: {err}");
        std::process::exit(1);
    }

    // === Parent: write secret to pipe, wait for child ===

    // Close read end.
    unsafe { libc::close(pipe_read) };

    // Close channel fd in parent — only the child needs it.
    if let Some(ch_fd) = channel_fd {
        unsafe { libc::close(ch_fd) };
    }

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
    /// Whether libsecret mode is enabled for this wrapper.
    libsecret: bool,
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
        #[serde(default)]
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
        libsecret: wf.libsecret,
        dbus_daemon_binary: wf.dbus_daemon_binary,
        secretsd_binary: wf.secretsd_binary,
        env_passthrough: wf.env_passthrough,
        env_set: wf.env_set,
    })
}

/// Request the secret from the daemon. If `libsecret` is true, also
/// receive the secretsd channel fd via SCM_RIGHTS.
/// Returns (secret_material, optional_channel_fd).
fn request_secret(
    wrapper_name: &str,
    socket_path: &str,
    libsecret: bool,
) -> Result<(String, Option<i32>)> {
    let mut stream = UnixStream::connect(socket_path)
        .with_context(|| format!("connecting to daemon at {socket_path}"))?;

    let scope = std::env::var("UNDERWRAP_SCOPE").ok();
    let req = WrapperRequest::Read {
        wrapper: wrapper_name.to_string(),
        scope,
    };
    write_frame_sync(&mut stream, &req)?;

    let response: WrapperResponse = read_frame_sync(&mut stream)?;
    let material = match response {
        WrapperResponse::Ok {
            material: Some(m), ..
        } => m,
        WrapperResponse::Ok { material: None, .. } => {
            bail!("daemon returned no material");
        }
        WrapperResponse::Error { code, message } => {
            bail!("daemon denied request ({code:?}): {message}");
        }
        WrapperResponse::Challenge { .. } => {
            bail!("unexpected challenge response for read request");
        }
    };

    // If libsecret is enabled, the daemon sends the secretsd channel fd
    // via SCM_RIGHTS immediately after the response.
    // Failure is fatal — no fallback to non-libsecret mode.
    let channel_fd = if libsecret {
        match underwrap_protocol::scm_rights::recv_fd(&stream) {
            Ok(fd) => Some(fd),
            Err(e) => {
                bail!("libsecret enabled but failed to receive secretsd channel fd: {e}");
            }
        }
    } else {
        None
    };

    Ok((material, channel_fd))
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
