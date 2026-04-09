//! underwrap-exec — the wrapper binary (stage 1).
//!
//! Installed via NixOS security.wrappers with setgid for GID-based identity.
//!
//! 1. Derives wrapper name from effective GID (not argv[0])
//! 2. Loads sidecar config from trusted path
//! 3. Connects to the wrapper socket, gets secret (daemon verifies egid)
//! 4. Drops all elevated privileges
//! 5. Forks: parent writes secret to pipe, child execs underwrap-ns
//! 6. underwrap-ns creates PID namespace and execs the target
//!
//! The secret passes through a pipe fd — never in env, argv, or filesystem
//! between the two stages.

use anyhow::{Context, Result, bail};
use std::os::unix::net::UnixStream;

use underwrap_protocol::exec::ExecBundle;
use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{SecretBytes, WrapperRequest, WrapperResponse};

/// Fixed config directory used when running with elevated privileges.
const TRUSTED_CONFIG_DIR: &str = "/etc/underwrap/wrappers";

/// Path to the underwrap-ns binary. Set by the NixOS module via sidecar config.
/// Falls back to searching next to the current executable.
const DEFAULT_NS_BINARY: &str = "/run/wrappers/bin/underwrap-ns";

fn main() -> Result<()> {
    // Derive wrapper identity from the kernel-assigned effective GID,
    // NOT from argv[0]. This prevents symlink confused-deputy attacks
    // where argv[0] could name a different wrapper's sidecar config.
    let wrapper_name = detect_wrapper_name_from_egid()?;
    let all_args: Vec<String> = std::env::args().skip(1).collect();

    // Check if this is an unlock request.
    if all_args.first().map(|s| s.as_str()) == Some("--unlock") {
        return handle_unlock(&wrapper_name, &all_args[1..]);
    }

    let wrapper_config = load_wrapper_config(&wrapper_name)?;

    // Request secret from daemon (while still setgid — daemon checks egid).
    // If libsecret mode, also receive the secretsd channel fd.
    let (env_bindings, channel_fd) =
        request_secret(&wrapper_config.wrapper_socket, wrapper_config.libsecret)?;

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
        channel_fd,
    )
}

/// Handle --unlock: request unlock via the wrapper socket with SSH agent challenge.
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
    let mut stream = UnixStream::connect(&wrapper_config.wrapper_socket)
        .with_context(|| format!("connecting to daemon at {}", wrapper_config.wrapper_socket))?;

    let req = WrapperRequest::Unlock {
        scope: scope.clone(),
        seconds,
    };
    write_frame_sync(&mut stream, &req)?;
    let resp: WrapperResponse = read_frame_sync(&mut stream)?;

    match resp {
        WrapperResponse::UnlockOk => {
            drop(stream);
            eprintln!("unlocked");
            drop_privilege()?;
            Ok(())
        }
        WrapperResponse::Challenge { nonce } => {
            let (signature, public_key_blob) = sign_with_ssh_agent(&nonce)?;

            let req2 = WrapperRequest::UnlockResponse {
                scope,
                seconds,
                challenge: nonce,
                signature,
                public_key_blob,
            };
            write_frame_sync(&mut stream, &req2)?;
            let resp2: WrapperResponse = read_frame_sync(&mut stream)?;
            drop(stream);

            drop_privilege()?;

            match resp2 {
                WrapperResponse::UnlockOk => {
                    eprintln!("unlocked");
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
        _ => bail!("unexpected response"),
    }
}

/// Sign a challenge nonce using the SSH agent (SSH_AUTH_SOCK).
fn sign_with_ssh_agent(nonce: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let auth_sock = std::env::var("SSH_AUTH_SOCK").context("SSH_AUTH_SOCK not set")?;
    let mut agent = UnixStream::connect(&auth_sock)
        .with_context(|| format!("connecting to SSH agent at {auth_sock}"))?;

    let identities = ssh_agent_request_identities(&mut agent)?;
    if identities.is_empty() {
        bail!("no keys in SSH agent");
    }

    let (key_blob, key_comment) = &identities[0];
    let signature = ssh_agent_sign(&mut agent, key_blob, nonce)?;

    eprintln!("signed challenge with key: {key_comment}");
    Ok((signature, key_blob.clone()))
}

// ---------------------------------------------------------------------------
// Minimal SSH agent protocol implementation
// ---------------------------------------------------------------------------

const SSH_AGENTC_REQUEST_IDENTITIES: u8 = 11;
const SSH_AGENT_IDENTITIES_ANSWER: u8 = 12;
const SSH_AGENTC_SIGN_REQUEST: u8 = 13;
const SSH_AGENT_SIGN_RESPONSE: u8 = 14;
const MAX_SSH_AGENT_MESSAGE_SIZE: usize = underwrap_protocol::ipc::MAX_FRAME_SIZE;

fn read_ssh_agent_message(stream: &mut impl std::io::Read) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let resp_len = u32::from_be_bytes(len_buf) as usize;
    anyhow::ensure!(
        resp_len <= MAX_SSH_AGENT_MESSAGE_SIZE,
        "SSH agent response too large: {resp_len} bytes"
    );
    let mut resp = vec![0u8; resp_len];
    stream.read_exact(&mut resp)?;
    Ok(resp)
}

fn take_ssh_string<'a>(buf: &'a [u8], pos: &mut usize, label: &str) -> Result<&'a [u8]> {
    anyhow::ensure!(
        *pos + 4 <= buf.len(),
        "truncated SSH agent {label} length field"
    );
    let len = u32::from_be_bytes([buf[*pos], buf[*pos + 1], buf[*pos + 2], buf[*pos + 3]]) as usize;
    *pos += 4;
    anyhow::ensure!(
        *pos + len <= buf.len(),
        "truncated SSH agent {label} payload"
    );
    let out = &buf[*pos..*pos + len];
    *pos += len;
    Ok(out)
}

fn parse_ssh_agent_identities_response(resp: &[u8]) -> Result<Vec<(Vec<u8>, String)>> {
    anyhow::ensure!(!resp.is_empty(), "empty SSH agent identities response");
    anyhow::ensure!(
        resp[0] == SSH_AGENT_IDENTITIES_ANSWER,
        "unexpected agent response type: {}",
        resp[0]
    );
    anyhow::ensure!(
        resp.len() >= 5,
        "truncated SSH agent identities response header"
    );

    let nkeys = u32::from_be_bytes([resp[1], resp[2], resp[3], resp[4]]) as usize;
    let mut pos = 5;
    let mut keys = Vec::with_capacity(nkeys.min(32));

    for _ in 0..nkeys {
        let blob = take_ssh_string(resp, &mut pos, "key blob")?.to_vec();
        let comment_bytes = take_ssh_string(resp, &mut pos, "key comment")?;
        let comment = String::from_utf8_lossy(comment_bytes).to_string();
        keys.push((blob, comment));
    }

    anyhow::ensure!(
        pos == resp.len(),
        "trailing bytes in SSH agent identities response"
    );
    Ok(keys)
}

fn ssh_agent_request_identities(agent: &mut UnixStream) -> Result<Vec<(Vec<u8>, String)>> {
    use std::io::Write;

    // Send request.
    let msg = [SSH_AGENTC_REQUEST_IDENTITIES];
    let len = (msg.len() as u32).to_be_bytes();
    agent.write_all(&len)?;
    agent.write_all(&msg)?;

    let resp = read_ssh_agent_message(agent)?;
    parse_ssh_agent_identities_response(&resp)
}

fn parse_ssh_agent_sign_response(resp: &[u8]) -> Result<Vec<u8>> {
    anyhow::ensure!(!resp.is_empty(), "empty SSH agent sign response");
    anyhow::ensure!(
        resp[0] == SSH_AGENT_SIGN_RESPONSE,
        "agent refused to sign (response type: {})",
        resp[0]
    );
    let mut pos = 1;
    let sig = take_ssh_string(resp, &mut pos, "signature blob")?.to_vec();
    anyhow::ensure!(
        pos == resp.len(),
        "trailing bytes in SSH agent sign response"
    );
    Ok(sig)
}

fn ssh_agent_sign(agent: &mut UnixStream, key_blob: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    use std::io::Write;

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

    let resp = read_ssh_agent_message(agent)?;
    parse_ssh_agent_sign_response(&resp)
}

/// Fork, pipe env secret bindings to the child, child execs underwrap-ns.
fn exec_via_ns(
    ns_binary: &str,
    config: &WrapperExecConfig,
    args: &[String],
    env_bindings: std::collections::HashMap<String, SecretBytes>,
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

        // Build argv for underwrap-ns: <ns_binary> <target> [args...]
        use std::os::unix::process::CommandExt;
        let mut cmd = std::process::Command::new(ns_binary);
        cmd.arg(&config.target);

        // ALWAYS strip internal env vars from the child — a malicious user
        // could set these to inject code into the PID namespace.
        cmd.env_remove("UNDERWRAP_LIBSECRET");
        cmd.env_remove("UNDERWRAP_DBUS_DAEMON");
        cmd.env_remove("UNDERWRAP_SECRETSD");
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

        cmd.args(args);

        let err = cmd.exec();
        eprintln!("underwrap-exec: exec underwrap-ns failed: {err}");
        std::process::exit(1);
    }

    // === Parent: write env bindings to pipe, wait for child ===

    // Close read end.
    unsafe { libc::close(pipe_read) };

    // Close channel fd in parent — only the child needs it.
    if let Some(ch_fd) = channel_fd {
        unsafe { libc::close(ch_fd) };
    }

    // Write env bindings to pipe.
    {
        use std::os::unix::io::FromRawFd;
        let mut pipe = unsafe { std::fs::File::from_raw_fd(pipe_write) };
        let payload = ExecBundle {
            env: env_bindings,
            env_passthrough: config.env_passthrough.clone(),
            env_set: config.env_set.clone(),
        };
        write_frame_sync(&mut pipe, &payload).context("writing env bindings to pipe")?;
        // pipe_write closed on drop.
    }

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

/// The prefix used by the NixOS module for wrapper group names.
const GROUP_PREFIX: &str = "underwrap-";

/// Derive wrapper name from the kernel-assigned effective GID.
/// This is the ONLY trusted source of wrapper identity. argv[0] is
/// user-controlled and could name a different wrapper (symlink attack).
fn detect_wrapper_name_from_egid() -> Result<String> {
    let egid = unsafe { libc::getegid() };
    let group_name = resolve_gid_name(egid)
        .with_context(|| format!("cannot resolve effective GID {egid} to a group name"))?;

    // Strip the "underwrap-" prefix to get the wrapper name.
    // The NixOS module creates groups as "underwrap-<instance>-<wrapper>".
    let wrapper_name = group_name
        .strip_prefix(GROUP_PREFIX)
        .with_context(|| {
            format!(
                "effective GID group '{group_name}' does not start with '{GROUP_PREFIX}' — \
                 not an underwrap wrapper group"
            )
        })?
        .to_string();

    Ok(wrapper_name)
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
        wrapper_socket: String,
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
        libsecret: wf.libsecret,
        dbus_daemon_binary: wf.dbus_daemon_binary,
        secretsd_binary: wf.secretsd_binary,
        env_passthrough: wf.env_passthrough,
        env_set: wf.env_set,
    })
}

/// Request credentials from the daemon. If `libsecret` is true, also
/// receive the secretsd channel fd via SCM_RIGHTS.
/// Returns (env_bindings, optional_channel_fd).
fn request_secret(
    socket_path: &str,
    libsecret: bool,
) -> Result<(std::collections::HashMap<String, SecretBytes>, Option<i32>)> {
    let mut stream = UnixStream::connect(socket_path)
        .with_context(|| format!("connecting to daemon at {socket_path}"))?;

    let scope = std::env::var("UNDERWRAP_SCOPE").ok();
    let req = WrapperRequest::Read { scope };
    write_frame_sync(&mut stream, &req)?;

    let response: WrapperResponse = read_frame_sync(&mut stream)?;
    let env_bindings = match response {
        WrapperResponse::ReadOk { env } => env,
        WrapperResponse::Error { code, message } => {
            bail!("daemon denied request ({code:?}): {message}");
        }
        WrapperResponse::Challenge { .. } | WrapperResponse::UnlockOk => {
            bail!("unexpected response for read request");
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

    Ok((env_bindings, channel_fd))
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
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_ssh_agent_message_rejects_oversized_response() {
        let oversized = (MAX_SSH_AGENT_MESSAGE_SIZE as u32 + 1).to_be_bytes();
        let mut cursor = Cursor::new(oversized.to_vec());
        let err = read_ssh_agent_message(&mut cursor).unwrap_err();
        assert!(err.to_string().contains("response too large"));
    }

    #[test]
    fn parse_ssh_agent_identities_response_rejects_truncated_blob() {
        let resp = vec![
            SSH_AGENT_IDENTITIES_ANSWER,
            0,
            0,
            0,
            1, // one key
            0,
            0,
            0,
            4, // blob len says 4 bytes
            1,
            2, // only 2 bytes present
        ];
        let err = parse_ssh_agent_identities_response(&resp).unwrap_err();
        assert!(
            err.to_string()
                .contains("truncated SSH agent key blob payload")
        );
    }

    #[test]
    fn parse_ssh_agent_sign_response_rejects_truncated_signature() {
        let resp = vec![
            SSH_AGENT_SIGN_RESPONSE,
            0,
            0,
            0,
            10, // signature length says 10 bytes
            1,
            2,
            3,
        ];
        let err = parse_ssh_agent_sign_response(&resp).unwrap_err();
        assert!(
            err.to_string()
                .contains("truncated SSH agent signature blob payload")
        );
    }

    #[test]
    fn parse_ssh_agent_sign_response_accepts_valid_signature_blob() {
        let resp = vec![SSH_AGENT_SIGN_RESPONSE, 0, 0, 0, 3, 9, 8, 7];
        let sig = parse_ssh_agent_sign_response(&resp).unwrap();
        assert_eq!(sig, vec![9, 8, 7]);
    }
}
