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

use crate::storage::Store;

/// Maximum concurrent connections per socket.
const MAX_CONNECTIONS: usize = 128;

/// Shared daemon state passed to connection handlers.
struct State {
    config: DaemonConfig,
    store: Store,
}

/// Run the daemon until `shutdown` is notified (or forever if `None`).
pub async fn run(config: DaemonConfig, shutdown: Option<Arc<Notify>>) -> Result<()> {
    // Validate config at startup: every wrapper.secret must reference a declared secret.
    validate_config(&config)?;

    let store = Store::open(&config.storage)?;
    let state = Arc::new(State {
        config: config.clone(),
        store,
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
#[allow(dead_code)] // pid used via Debug formatting in tracing.
struct PeerCred {
    pid: u32,
    uid: u32,
    gid: u32,
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
        Some(PeerCred {
            pid: cred.pid as u32,
            uid: cred.uid,
            gid: cred.gid,
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
        warn!(peer = ?peer, reason = %reason, "read auth denied");
        let response = ReadResponse::Error {
            code: ErrorCode::Unauthorized,
            message: reason,
        };
        write_frame_async(&mut stream, &response).await?;
        stream.shutdown().await?;
        return Ok(());
    }

    let request: ReadRequest = read_frame_async(&mut stream).await?;
    info!(
        wrapper = %request.wrapper,
        peer = ?peer,
        "read request"
    );

    let response = process_read(&request, &state);

    write_frame_async(&mut stream, &response).await?;
    stream.shutdown().await?;
    Ok(())
}

fn process_read(request: &ReadRequest, state: &State) -> ReadResponse {
    let wrapper_def = match state.config.wrappers.get(&request.wrapper) {
        Some(w) => w,
        None => {
            warn!(wrapper = %request.wrapper, "unknown wrapper");
            return ReadResponse::Error {
                code: ErrorCode::NotFound,
                message: format!("unknown wrapper '{}'", request.wrapper),
            };
        }
    };

    let secret_id = &wrapper_def.secret;

    match state.store.read(secret_id) {
        Ok(Some(material)) => {
            info!(wrapper = %request.wrapper, secret = %secret_id, "read granted");
            ReadResponse::Ok { material }
        }
        Ok(None) => {
            warn!(secret = %secret_id, "secret not found in store");
            ReadResponse::Error {
                code: ErrorCode::NotFound,
                message: format!("secret '{secret_id}' not found in store"),
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
        warn!(peer = ?peer, reason = %reason, "control auth denied");
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
    }

    let response = process_control(&request, &state);

    write_frame_async(&mut stream, &response).await?;
    stream.shutdown().await?;
    Ok(())
}

fn process_control(request: &ControlRequest, state: &State) -> ControlResponse {
    match request {
        ControlRequest::Status { target } => {
            let info = build_status(target, state);
            ControlResponse::Ok { info: Some(info) }
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

            // Enforce mutability: if mutable=false and secret already exists, reject.
            if !secret_def.mutable
                && let Ok(Some(_)) = state.store.read(secret_id)
            {
                warn!(secret = %secret_id, "rejected overwrite of immutable secret");
                return ControlResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: format!("secret '{secret_id}' is immutable and already exists"),
                };
            }

            match state.store.write(secret_id, material) {
                Ok(()) => {
                    info!(secret = %secret_id, "secret stored");
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
                let stored = state.store.read(&w.secret).ok().flatten().is_some();
                serde_json::json!({
                    "wrapper": name,
                    "target": w.target,
                    "secret": w.secret,
                    "secret_stored": stored,
                })
            } else if state.config.secrets.contains_key(name) {
                let stored = state.store.read(name).ok().flatten().is_some();
                serde_json::json!({
                    "secret": name,
                    "stored": stored,
                })
            } else {
                serde_json::json!({
                    "error": format!("unknown target '{name}'"),
                })
            }
        }
    }
}
