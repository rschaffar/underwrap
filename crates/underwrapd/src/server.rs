//! Unix socket server for underwrapd.
//!
//! Runs two listeners concurrently:
//! - read socket: wrappers request secret material
//! - control socket: underwrapctl stores secrets, queries status

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Notify, Semaphore};
use tracing::{error, info, warn};

use underwrap_protocol::config::DaemonConfig;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::*;

use crate::audit;
use crate::derive::{self, TokenCache};
use crate::unlock::UnlockState;
use underwrap_store::StorageBackend;

/// Maximum concurrent connections per socket.
const MAX_CONNECTIONS: usize = 128;

/// Shared daemon state passed to connection handlers.
struct State {
    config: DaemonConfig,
    store: Box<dyn StorageBackend>,
    unlock: UnlockState,
    token_cache: TokenCache,
    /// GID → wrapper name lookup for kernel-verified identity.
    /// Built at startup from wrappers that have `group` set.
    gid_to_wrapper: std::collections::HashMap<u32, String>,
    /// Per-secret write serialization to prevent immutability TOCTOU.
    write_locks: std::sync::Mutex<std::collections::HashSet<String>>,
}

/// Run the daemon until `shutdown` is notified (or forever if `None`).
pub async fn run(config: DaemonConfig, shutdown: Option<Arc<Notify>>) -> Result<()> {
    // Validate config at startup: every wrapper.secret must reference a declared secret.
    validate_config(&config)?;

    let store = underwrap_store::open(&config.storage)?;
    let gid_to_wrapper = build_gid_map(&config)?;
    let state = Arc::new(State {
        config: config.clone(),
        store,
        unlock: UnlockState::new(),
        token_cache: TokenCache::new(),
        gid_to_wrapper,
        write_locks: std::sync::Mutex::new(std::collections::HashSet::new()),
    });

    let read_listener = bind_socket(&config.sockets.read, config.sockets.read_mode, "read")?;
    let control_listener = bind_socket(
        &config.sockets.control,
        config.sockets.control_mode,
        "control",
    )?;

    info!("daemon ready");

    match shutdown {
        Some(notify) => {
            tokio::select! {
                r = accept_loop(read_listener, Arc::clone(&state), handle_read) => r,
                r = accept_loop(control_listener, state, handle_control) => r,
                _ = notify.notified() => {
                    info!("shutdown signal received");
                    Ok(())
                }
            }
        }
        None => {
            tokio::select! {
                r = accept_loop(read_listener, Arc::clone(&state), handle_read) => r,
                r = accept_loop(control_listener, state, handle_control) => r,
            }
        }
    }
}

/// Build a GID → wrapper-name lookup table.
/// Resolves group names to GIDs via libc getgrnam at startup.
fn build_gid_map(config: &DaemonConfig) -> Result<std::collections::HashMap<u32, String>> {
    let mut map = std::collections::HashMap::new();
    for (name, wrapper) in &config.wrappers {
        let group_name = &wrapper.group;
        let gid = resolve_group_gid(group_name)
            .with_context(|| format!("resolving group '{group_name}' for wrapper '{name}'"))?;
        if let Some(existing) = map.get(&gid) {
            warn!(
                gid = gid,
                existing = %existing,
                new = %name,
                "multiple wrappers share GID — last one wins (wrapper isolation is weakened)"
            );
        }
        info!(wrapper = %name, group = %group_name, gid = gid, "resolved wrapper group");
        map.insert(gid, name.clone());
    }
    Ok(map)
}

/// Resolve a Unix group name to a GID.
fn resolve_group_gid(name: &str) -> Result<u32> {
    let c_name = std::ffi::CString::new(name)
        .with_context(|| format!("group name '{name}' contains null byte"))?;
    let grp = unsafe { libc::getgrnam(c_name.as_ptr()) };
    if grp.is_null() {
        anyhow::bail!("group '{name}' not found");
    }
    Ok(unsafe { (*grp).gr_gid })
}

/// Validate configuration at load time.
fn validate_config(config: &DaemonConfig) -> Result<()> {
    for (name, wrapper) in &config.wrappers {
        if !config.secrets.contains_key(&wrapper.secret) {
            anyhow::bail!(
                "wrapper '{}' references undeclared secret '{}'",
                name,
                wrapper.secret
            );
        }
    }
    Ok(())
}

fn bind_socket(path: &std::path::Path, mode: u32, label: &str) -> Result<UnixListener> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {label} socket dir {}", parent.display()))?;
    }
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)
        .with_context(|| format!("binding {label} socket {}", path.display()))?;

    // Explicitly set socket permissions.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .with_context(|| format!("setting {label} socket permissions to {mode:#o}"))?;
    }

    info!(path = %path.display(), mode = format!("{mode:#o}"), "{label} socket listening");
    Ok(listener)
}

async fn accept_loop<F, Fut>(listener: UnixListener, state: Arc<State>, handler: F) -> Result<()>
where
    F: Fn(UnixStream, Arc<State>) -> Fut + Send + Sync + Copy + 'static,
    Fut: std::future::Future<Output = Result<()>> + Send,
{
    let semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS));

    loop {
        let (stream, _addr) = listener.accept().await?;
        let state = Arc::clone(&state);
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore closed");

        tokio::spawn(async move {
            if let Err(e) = handler(stream, state).await {
                error!("connection error: {e:#}");
            }
            drop(permit);
        });
    }
}

// ---------------------------------------------------------------------------
// Peer credentials (SO_PEERCRED)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used via Debug formatting in tracing.
struct PeerCred {
    pid: u32,
    uid: u32,
    gid: u32,
    /// Effective GID, read from /proc/<pid>/status.
    /// This is the one set by setgid wrappers.
    /// SO_PEERCRED.gid only reports the *real* GID.
    egid: Option<u32>,
}

fn get_peer_cred(stream: &UnixStream) -> Option<PeerCred> {
    use std::os::unix::io::AsRawFd;
    let std_fd = stream.as_raw_fd();
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            std_fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if ret == 0 {
        let egid = read_proc_egid(cred.pid as u32);
        Some(PeerCred {
            pid: cred.pid as u32,
            uid: cred.uid,
            gid: cred.gid,
            egid,
        })
    } else {
        warn!("failed to get SO_PEERCRED");
        None
    }
}

/// Check whether a peer is authorized based on UID/GID allowlists.
/// If both lists are empty, all peers are allowed (backwards compat / dev mode).
fn check_peer_auth(
    peer: &Option<PeerCred>,
    allow_uids: &[u32],
    allow_gids: &[u32],
) -> std::result::Result<(), String> {
    // If no restrictions configured, allow all.
    if allow_uids.is_empty() && allow_gids.is_empty() {
        return Ok(());
    }

    let cred = match peer {
        Some(c) => c,
        None => return Err("could not determine peer credentials".to_string()),
    };

    // UID 0 (root) always allowed.
    if cred.uid == 0 {
        return Ok(());
    }

    if !allow_uids.is_empty() && allow_uids.contains(&cred.uid) {
        return Ok(());
    }

    if !allow_gids.is_empty() && allow_gids.contains(&cred.gid) {
        return Ok(());
    }

    Err(format!(
        "peer uid={} gid={} not in allowlist",
        cred.uid, cred.gid
    ))
}

/// Verify that the peer process is running the expected executable.
/// Reads /proc/<pid>/exe to get the real binary path.
fn check_peer_exe(
    peer: &Option<PeerCred>,
    allowed_exe: &std::path::Path,
) -> std::result::Result<(), String> {
    let cred = match peer {
        Some(c) => c,
        None => return Err("could not determine peer credentials for exe check".to_string()),
    };

    let exe_link = format!("/proc/{}/exe", cred.pid);
    match std::fs::read_link(&exe_link) {
        Ok(exe_path) => {
            if exe_path == allowed_exe {
                Ok(())
            } else {
                Err(format!(
                    "peer exe {} does not match allowed {}",
                    exe_path.display(),
                    allowed_exe.display()
                ))
            }
        }
        Err(e) => Err(format!("cannot read {exe_link}: {e}")),
    }
}

/// Read the effective GID from /proc/<pid>/status.
/// Returns None if the file can't be read (e.g. process already exited).
fn read_proc_egid(pid: u32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        // Format: "Gid:\treal\teffective\tsaved\tfs"
        if let Some(rest) = line.strip_prefix("Gid:\t") {
            let fields: Vec<&str> = rest.split('\t').collect();
            if fields.len() >= 2 {
                return fields[1].parse().ok();
            }
        }
    }
    None
}

/// Resolve wrapper identity strictly from the peer's effective GID.
///
/// The daemon does NOT trust client-supplied wrapper names. Identity comes
/// from the kernel: the peer's effective GID (set by the setgid wrapper
/// binary) is looked up in the GID→wrapper map built at startup.
///
/// Returns None if the peer's egid doesn't map to any configured wrapper.
fn resolve_wrapper_identity(
    peer: &Option<PeerCred>,
    gid_map: &std::collections::HashMap<u32, String>,
) -> Option<String> {
    let cred = peer.as_ref()?;
    let egid = cred.egid?;
    gid_map.get(&egid).cloned()
}

// ---------------------------------------------------------------------------
// Read socket handler
// ---------------------------------------------------------------------------

async fn handle_read(mut stream: UnixStream, state: Arc<State>) -> Result<()> {
    let peer = get_peer_cred(&stream);

    // Auth check for read socket.
    if let Err(reason) = check_peer_auth(
        &peer,
        &state.config.sockets.read_allow_uids,
        &state.config.sockets.read_allow_gids,
    ) {
        audit::auth_denied(
            "read",
            &reason,
            peer.as_ref().map(|p| p.uid),
            peer.as_ref().map(|p| p.gid),
        );
        let response = ReadResponse::Error {
            code: ErrorCode::Unauthorized,
            message: reason,
        };
        write_frame_async(&mut stream, &response).await?;
        stream.shutdown().await?;
        return Ok(());
    }

    // Verify peer executable if configured.
    if let Some(ref allowed_exe) = state.config.sockets.read_allow_exe
        && let Err(reason) = check_peer_exe(&peer, allowed_exe)
    {
        audit::auth_denied(
            "read",
            &reason,
            peer.as_ref().map(|p| p.uid),
            peer.as_ref().map(|p| p.gid),
        );
        let response = ReadResponse::Error {
            code: ErrorCode::Unauthorized,
            message: reason,
        };
        write_frame_async(&mut stream, &response).await?;
        stream.shutdown().await?;
        return Ok(());
    }

    let request: ReadRequest = read_frame_async(&mut stream).await?;

    // Resolve wrapper identity strictly from the peer's effective GID.
    // The client-supplied wrapper name is NOT trusted.
    let wrapper_name = match resolve_wrapper_identity(&peer, &state.gid_to_wrapper) {
        Some(name) => name,
        None => {
            let reason = format!(
                "peer egid {:?} does not map to any configured wrapper",
                peer.as_ref().and_then(|c| c.egid)
            );
            warn!(peer = ?peer, "wrapper identity rejected");
            audit::auth_denied(
                "read",
                &reason,
                peer.as_ref().map(|p| p.uid),
                peer.as_ref().map(|p| p.gid),
            );
            let response = ReadResponse::Error {
                code: ErrorCode::Unauthorized,
                message: reason,
            };
            write_frame_async(&mut stream, &response).await?;
            stream.shutdown().await?;
            return Ok(());
        }
    };

    info!(
        wrapper = %wrapper_name,
        peer = ?peer,
        "read request (identity from egid)"
    );

    let peer_uid = peer.as_ref().map(|p| p.uid);
    let response =
        process_read_for(&wrapper_name, request.scope.as_deref(), &state, peer_uid).await;

    write_frame_async(&mut stream, &response).await?;
    stream.shutdown().await?;
    Ok(())
}

async fn process_read_for(
    wrapper_name: &str,
    scope: Option<&str>,
    state: &State,
    peer_uid: Option<u32>,
) -> ReadResponse {
    let wrapper_def = match state.config.wrappers.get(wrapper_name) {
        Some(w) => w,
        None => {
            warn!(wrapper = %wrapper_name, "unknown wrapper");
            return ReadResponse::Error {
                code: ErrorCode::NotFound,
                message: format!("unknown wrapper '{wrapper_name}'"),
            };
        }
    };

    // Check unlock state if wrapper requires it.
    if wrapper_def.require_unlock
        && let Err(reason) = state.unlock.check(wrapper_name, scope, peer_uid)
    {
        audit::read_denied(wrapper_name, &reason, peer_uid, scope);
        return ReadResponse::Error {
            code: ErrorCode::Locked,
            message: reason,
        };
    }

    let secret_id = &wrapper_def.secret;

    match state.store.read(secret_id) {
        Ok(Some(material)) => {
            // Apply derivation (e.g. OAuth refresh → access token).
            match derive::derive(
                &wrapper_def.derivation,
                secret_id,
                &material,
                &state.token_cache,
            )
            .await
            {
                Ok(derived) => {
                    audit::read_granted(wrapper_name, secret_id, peer_uid, scope);
                    ReadResponse::Ok { material: derived }
                }
                Err(e) => {
                    error!(wrapper = %wrapper_name, secret = %secret_id, err = %e, "derivation failed");
                    ReadResponse::Error {
                        code: ErrorCode::DerivationFailed,
                        message: format!("derivation failed: {e}"),
                    }
                }
            }
        }
        Ok(None) => {
            let msg = format!("secret '{secret_id}' not found in store");
            audit::read_denied(wrapper_name, &msg, peer_uid, scope);
            ReadResponse::Error {
                code: ErrorCode::NotFound,
                message: msg,
            }
        }
        Err(e) => {
            error!(secret = %secret_id, err = %e, "storage read error");
            ReadResponse::Error {
                code: ErrorCode::Internal,
                message: "internal storage error".to_string(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Control socket handler
// ---------------------------------------------------------------------------

async fn handle_control(mut stream: UnixStream, state: Arc<State>) -> Result<()> {
    let peer = get_peer_cred(&stream);

    // Auth check for control socket.
    if let Err(reason) = check_peer_auth(
        &peer,
        &state.config.sockets.control_allow_uids,
        &state.config.sockets.control_allow_gids,
    ) {
        audit::auth_denied(
            "control",
            &reason,
            peer.as_ref().map(|p| p.uid),
            peer.as_ref().map(|p| p.gid),
        );
        let response = ControlResponse::Error {
            code: ErrorCode::Unauthorized,
            message: reason,
        };
        write_frame_async(&mut stream, &response).await?;
        stream.shutdown().await?;
        return Ok(());
    }

    let request: ControlRequest = read_frame_async(&mut stream).await?;

    // Log the command type but NEVER the secret material.
    match &request {
        ControlRequest::Status { target } => {
            info!(command = "status", target = ?target, peer = ?peer, "control request");
        }
        ControlRequest::Store { secret_id, .. } => {
            info!(command = "store", secret_id = %secret_id, peer = ?peer, "control request");
        }
        ControlRequest::Unlock {
            target,
            scope,
            seconds,
            ..
        } => {
            info!(command = "unlock", target = %target, scope = ?scope, seconds = ?seconds, peer = ?peer, "control request");
        }
        ControlRequest::Lock { target, scope } => {
            info!(command = "lock", target = ?target, scope = ?scope, peer = ?peer, "control request");
        }
    }

    let peer_uid = peer.as_ref().map(|p| p.uid);
    let response = process_control(&request, &state, peer_uid);

    write_frame_async(&mut stream, &response).await?;
    stream.shutdown().await?;
    Ok(())
}

fn process_control(
    request: &ControlRequest,
    state: &State,
    peer_uid: Option<u32>,
) -> ControlResponse {
    match request {
        ControlRequest::Status { target } => {
            let info = build_status(target, state);
            ControlResponse::Ok { info: Some(info) }
        }
        ControlRequest::Unlock {
            target,
            scope,
            seconds,
        } => {
            if !state.config.wrappers.contains_key(target) {
                return ControlResponse::Error {
                    code: ErrorCode::NotFound,
                    message: format!("unknown wrapper '{target}'"),
                };
            }
            // Root (uid=0) unlock grants cover all UIDs (admin grant).
            let grant_uid = if peer_uid == Some(0) { None } else { peer_uid };
            state
                .unlock
                .unlock(target, scope.as_deref(), *seconds, grant_uid);
            audit::unlocked(target, scope.as_deref(), *seconds, peer_uid);
            ControlResponse::Ok { info: None }
        }
        ControlRequest::Lock { target, scope } => {
            state.unlock.lock(target.as_deref(), scope.as_deref());
            audit::locked(target.as_deref(), scope.as_deref(), peer_uid);
            ControlResponse::Ok { info: None }
        }
        ControlRequest::Store {
            secret_id,
            material,
        } => {
            // Check that the secret is declared in config.
            let secret_def = match state.config.secrets.get(secret_id) {
                Some(def) => def,
                None => {
                    return ControlResponse::Error {
                        code: ErrorCode::NotFound,
                        message: format!("secret '{secret_id}' not declared in configuration"),
                    };
                }
            };

            // Serialize writes per secret to prevent immutability TOCTOU.
            {
                let mut locks = state.write_locks.lock().unwrap();
                if !locks.insert(secret_id.clone()) {
                    // Another concurrent write is in progress for this secret.
                    return ControlResponse::Error {
                        code: ErrorCode::Internal,
                        message: format!("concurrent write to '{secret_id}'"),
                    };
                }
            }

            // Now we hold the per-secret lock. Check + write atomically.
            let result = (|| {
                if !secret_def.mutable
                    && let Ok(Some(_)) = state.store.read(secret_id)
                {
                    let reason = "immutable and already exists";
                    audit::store_denied(secret_id, reason, peer_uid);
                    return ControlResponse::Error {
                        code: ErrorCode::Unauthorized,
                        message: format!("secret '{secret_id}' is {reason}"),
                    };
                }

                match state.store.write(secret_id, material) {
                    Ok(()) => {
                        state.token_cache.invalidate(secret_id);
                        audit::secret_stored(secret_id, peer_uid);
                        ControlResponse::Ok { info: None }
                    }
                    Err(e) => {
                        error!(secret = %secret_id, err = %e, "storage write error");
                        ControlResponse::Error {
                            code: ErrorCode::Internal,
                            message: "internal storage error".to_string(),
                        }
                    }
                }
            })();

            // Release per-secret lock.
            {
                let mut locks = state.write_locks.lock().unwrap();
                locks.remove(secret_id);
            }

            result
        }
    }
}

fn build_status(target: &Option<String>, state: &State) -> serde_json::Value {
    match target {
        None => {
            let mut wrappers: Vec<&String> = state.config.wrappers.keys().collect();
            wrappers.sort();
            let mut secrets: Vec<&String> = state.config.secrets.keys().collect();
            secrets.sort();
            serde_json::json!({
                "wrappers": wrappers,
                "secrets": secrets,
            })
        }
        Some(name) => {
            if let Some(w) = state.config.wrappers.get(name) {
                let (stored, storage_error) = match state.store.read(&w.secret) {
                    Ok(Some(_)) => (true, None),
                    Ok(None) => (false, None),
                    Err(e) => (false, Some(format!("{e}"))),
                };
                let unlock_status = state.unlock.status(name);
                let mut status = serde_json::json!({
                    "wrapper": name,
                    "target": w.target,
                    "secret": w.secret,
                    "secret_stored": stored,
                    "require_unlock": w.require_unlock,
                    "unlock": unlock_status,
                });
                if let Some(err) = storage_error {
                    status["storage_error"] = serde_json::json!(err);
                }
                status
            } else if state.config.secrets.contains_key(name) {
                let (stored, storage_error) = match state.store.read(name) {
                    Ok(Some(_)) => (true, None),
                    Ok(None) => (false, None),
                    Err(e) => (false, Some(format!("{e}"))),
                };
                let mut status = serde_json::json!({
                    "secret": name,
                    "stored": stored,
                });
                if let Some(err) = storage_error {
                    status["storage_error"] = serde_json::json!(err);
                }
                status
            } else {
                serde_json::json!({
                    "error": format!("unknown target '{name}'"),
                })
            }
        }
    }
}
