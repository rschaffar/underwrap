//! Unix socket server for underwrapd.
//!
//! Runs two listeners concurrently:
//! - read socket: wrappers request secret material
//! - control socket: underwrapctl stores secrets, queries status

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;
use tracing::{error, info, warn};

use underwrap_protocol::config::DaemonConfig;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::*;

use crate::storage::Store;

/// Shared daemon state passed to connection handlers.
struct State {
    config: DaemonConfig,
    store: Store,
}

/// Run the daemon until `shutdown` is notified (or forever if `None`).
pub async fn run(config: DaemonConfig, shutdown: Option<Arc<Notify>>) -> Result<()> {
    let store = Store::open(&config.storage)?;
    let state = Arc::new(State {
        config: config.clone(),
        store,
    });

    let read_listener = bind_socket(&config.sockets.read, "read")?;
    let control_listener = bind_socket(&config.sockets.control, "control")?;

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

fn bind_socket(path: &std::path::Path, label: &str) -> Result<UnixListener> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {label} socket dir {}", parent.display()))?;
    }
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)
        .with_context(|| format!("binding {label} socket {}", path.display()))?;
    info!(path = %path.display(), "{label} socket listening");
    Ok(listener)
}

async fn accept_loop<F, Fut>(listener: UnixListener, state: Arc<State>, handler: F) -> Result<()>
where
    F: Fn(UnixStream, Arc<State>) -> Fut + Send + Sync + Copy + 'static,
    Fut: std::future::Future<Output = Result<()>> + Send,
{
    loop {
        let (stream, _addr) = listener.accept().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handler(stream, state).await {
                error!("connection error: {e:#}");
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Peer credentials (SO_PEERCRED)
// ---------------------------------------------------------------------------

#[derive(Debug)]
#[allow(dead_code)] // Fields are read via Debug formatting in tracing.
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

// ---------------------------------------------------------------------------
// Read socket handler
// ---------------------------------------------------------------------------

async fn handle_read(mut stream: UnixStream, state: Arc<State>) -> Result<()> {
    let peer = get_peer_cred(&stream);

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

    // Phase 1 policy: wrapper may read only its declared secret.
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

    let request: ControlRequest = read_frame_async(&mut stream).await?;
    info!(command = ?request, peer = ?peer, "control request");

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
            // Phase 1: check that the secret is declared in config.
            if !state.config.secrets.contains_key(secret_id) {
                return ControlResponse::Error {
                    code: ErrorCode::NotFound,
                    message: format!("secret '{secret_id}' not declared in configuration"),
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
