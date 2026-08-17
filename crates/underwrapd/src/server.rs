//! Unix socket server for underwrapd.
//!
//! Runs wrapper socket listeners. The daemon is a thin client of the
//! central backend: it only holds the YAML config and a lazily-opened
//! [`BackendClient`]. All policy evaluation, blob storage, and
//! writability enforcement live on the backend; the daemon relays
//! between the local wrapper socket and the runtime HTTPS API.

use anyhow::Result;
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinSet;
use tracing::{error, info, warn};

use underwrap_protocol::config::DaemonConfig;
use underwrap_protocol::service::{
    RuntimeWrapperRegisterPair, RuntimeWrapperRegisterRequest, RuntimeWrapperRegisterResponse,
};

use crate::backend_client::BackendClient;
use crate::startup::{
    bind_socket, check_credential_permissions, next_listener_result, resolve_instance_gid,
    validate_config,
};
use crate::wrapper_flow::handle_wrapper;

/// Maximum concurrent connections per socket.
const MAX_CONNECTIONS: usize = 128;

/// Shared daemon state passed to connection handlers.
///
/// The [`BackendClient`] is built lazily on the first wrapper request
/// that needs it. A transient backend hiccup at boot, or a missing
/// credential file at boot, does not pin the daemon into a permanent
/// "unenrolled" mode — every wrapper request retries the open if the
/// previous attempt failed.
pub(crate) struct State {
    pub(crate) config: DaemonConfig,
    backend: std::sync::Mutex<Option<Arc<BackendClient>>>,
}

#[derive(Debug)]
pub(crate) enum BackendOpenError {
    /// The combined identity configured for this instance isn't on disk yet.
    /// The operator hasn't run `underwrap-keygen` for this instance, or the
    /// keygen output landed somewhere else.
    MissingCredential,
    /// The configured identity or CA could not be read/parsed, or the HTTPS
    /// client could not be constructed. Network/authentication failures occur
    /// only when that client later issues a runtime request.
    Open(anyhow::Error),
}

impl State {
    /// Build a fresh, "unopened" `State`. The backend client is
    /// constructed lazily on the first wrapper request that needs it.
    pub(crate) fn new(config: DaemonConfig) -> Self {
        Self {
            config,
            backend: std::sync::Mutex::new(None),
        }
    }

    /// Return the URL the wrapper should send the operator to when the
    /// daemon can't connect to its backend.
    pub(crate) fn enrollment_url(&self) -> String {
        build_enrollment_url(&self.config)
    }

    /// Get (or lazily build) the [`BackendClient`]. Construction failures are
    /// not cached: a later wrapper request retries after the operator installs
    /// or repairs the identity/CA files.
    pub(crate) fn ensure_backend(&self) -> Result<Arc<BackendClient>, BackendOpenError> {
        let mut guard = self.backend.lock().unwrap();
        if let Some(c) = guard.as_ref() {
            return Ok(Arc::clone(c));
        }
        let credential_on_disk = self
            .config
            .backend
            .client_identity_file
            .as_ref()
            .is_some_and(|identity| identity.exists());
        if !credential_on_disk {
            return Err(BackendOpenError::MissingCredential);
        }
        let client = BackendClient::open(&self.config.backend).map_err(BackendOpenError::Open)?;
        let arc = Arc::new(client);
        *guard = Some(Arc::clone(&arc));
        Ok(arc)
    }
}

/// Render `(wrapper, resource)` pairs as a stable, comma-separated
/// `wrapper=>resource` list for a log field. Empty input renders as the
/// empty string.
fn format_register_pairs(pairs: &[RuntimeWrapperRegisterPair]) -> String {
    pairs
        .iter()
        .map(|p| format!("{}=>{}", p.wrapper_name, p.resource_name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Register every configured wrapper's `(wrapper, resource)` pair
/// with the backend. Logs the names of any resources/pairs the backend
/// auto-created, or a warning naming what it refused to create when
/// auto-create is disabled for this client instance. Best-effort: a
/// failure here does not stop the daemon — the open call for each
/// invocation will re-issue the appropriate error.
async fn register_wrapper_assets(backend: &BackendClient, config: &DaemonConfig) -> Result<()> {
    let mut wrappers = Vec::with_capacity(config.wrappers.len());
    for (wrapper_name, wrapper) in &config.wrappers {
        wrappers.push(RuntimeWrapperRegisterPair {
            wrapper_name: wrapper_name.clone(),
            resource_name: wrapper.resource.clone(),
        });
    }
    let response = backend
        .register(RuntimeWrapperRegisterRequest { wrappers })
        .await?;
    match response {
        RuntimeWrapperRegisterResponse::Ok {
            auto_created_resources,
            auto_created_pairs,
        } => {
            if !auto_created_resources.is_empty() || !auto_created_pairs.is_empty() {
                info!(
                    created_resource_count = auto_created_resources.len(),
                    created_pair_count = auto_created_pairs.len(),
                    created_resources = %auto_created_resources.join(", "),
                    created_pairs = %format_register_pairs(&auto_created_pairs),
                    "backend auto-created wrapper assets on register",
                );
            }
        }
        RuntimeWrapperRegisterResponse::AutoCreateDisabled {
            missing_resources,
            missing_pairs,
        } => {
            warn!(
                missing_resource_count = missing_resources.len(),
                missing_pair_count = missing_pairs.len(),
                missing_resources = %missing_resources.join(", "),
                missing_pairs = %format_register_pairs(&missing_pairs),
                "backend refused wrapper register: auto-create disabled for this client instance — \
                 an admin must enable `auto_create_wrapper_assets` on this instance, or create the \
                 missing resources and pairs (named above) manually",
            );
        }
    }
    Ok(())
}

fn build_enrollment_url(config: &DaemonConfig) -> String {
    format!(
        "{}/client-instances",
        config.backend.url.trim_end_matches('/'),
    )
}

pub(crate) fn unenrolled_message(url: &str) -> String {
    format!(
        "this underwrap instance is not enrolled yet. open the enrollment page in your browser: {url}"
    )
}

pub async fn run(config: DaemonConfig, shutdown: Option<Arc<Notify>>) -> Result<()> {
    validate_config(&config)?;
    check_credential_permissions(&config)?;
    crate::peer_auth::log_peer_auth_mode();

    let instance_gid = resolve_instance_gid(&config)?;
    let state = Arc::new(State::new(config.clone()));

    // Best-effort startup registration. Wrapper requests lazily retry opening
    // the backend client, but they do not repeat this registration call; after
    // enrollment or a startup registration failure, the operator restarts the
    // daemon to register configured assets.
    match state.ensure_backend() {
        Ok(client) => {
            if let Err(e) = register_wrapper_assets(&client, &config).await {
                warn!(
                    err = %format!("{e:#}"),
                    "wrapper-asset register call failed at startup; restart the daemon to retry registration",
                );
            } else {
                info!("backend client opened at startup");
            }
        }
        Err(BackendOpenError::MissingCredential) => info!(
            "client identity not yet on disk; daemon will retry backend client construction on the next wrapper request",
        ),
        Err(BackendOpenError::Open(e)) => warn!(
            err = %format!("{e:#}"),
            "backend client construction failed at startup; daemon will retry on the next wrapper request",
        ),
    }

    let mut tasks = JoinSet::new();

    let listener = bind_socket(
        &config.socket,
        0o660,
        &format!("instance '{}'", config.instance),
        Some(instance_gid),
    )?;
    tasks.spawn(accept_wrapper_loop(
        listener,
        Arc::clone(&state),
        instance_gid,
    ));

    info!("daemon ready");

    match shutdown {
        Some(notify) => {
            tokio::select! {
                r = next_listener_result(&mut tasks) => r,
                _ = notify.notified() => {
                    info!("shutdown signal received");
                    tasks.abort_all();
                    while tasks.join_next().await.is_some() {}
                    Ok(())
                }
            }
        }
        None => next_listener_result(&mut tasks).await,
    }
}

async fn accept_wrapper_loop(
    listener: UnixListener,
    state: Arc<State>,
    expected_gid: u32,
) -> Result<()> {
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
            if let Err(e) = handle_wrapper(stream, state, expected_gid).await {
                error!("connection error: {e:#}");
            }
            drop(permit);
        });
    }
}

#[cfg(test)]
mod tests;
