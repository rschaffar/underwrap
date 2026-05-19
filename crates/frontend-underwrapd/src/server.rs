//! Unix socket server for underwrapd.
//!
//! Runs wrapper socket listeners.

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinSet;
use tracing::{error, info, warn};

use underwrap_protocol::config::DaemonConfig;
use underwrap_protocol::service::{
    ServiceWrapperRegisterRequest, ServiceWrapperRegisterReject, ServiceWrapperResourcePair,
};

use crate::approval::PendingApprovals;
use crate::startup::{bind_socket, build_wrapper_gid_map, next_listener_result, validate_config};
use crate::wrapper_flow::handle_wrapper;
use underwrap_store::StorageBackend;

/// Maximum concurrent connections per socket.
const MAX_CONNECTIONS: usize = 128;

/// Shared daemon state passed to connection handlers.
pub(crate) struct State {
    pub(crate) config: DaemonConfig,
    pub(crate) store: Option<Arc<dyn StorageBackend>>,
    pub(crate) enrollment_url: Option<String>,
    /// The backend-assigned runtime client instance id this daemon
    /// authenticates as over mTLS. Empty for unenrolled instances —
    /// the daemon refuses wrapper requests in that state anyway, so
    /// the field is only consulted on the enrolled path.
    pub(crate) client_instance: String,
    pub(crate) pending_approvals: PendingApprovals,
}

impl State {
    /// Get a clone of the storage backend Arc for sharing with channel handlers.
    pub(crate) fn store_arc(&self) -> Arc<dyn StorageBackend> {
        Arc::clone(
            self.store
                .as_ref()
                .expect("storage backend is only available for enrolled instances"),
        )
    }
}

fn register_wrapper_assets(store: &dyn StorageBackend, config: &DaemonConfig) -> Result<()> {
    let mut pairs = Vec::new();
    for (wrapper_name, wrapper) in &config.wrappers {
        let logical_name = wrapper
            .logical_name
            .clone()
            .unwrap_or_else(|| wrapper_name.clone());
        for resource in wrapper.resources.keys() {
            pairs.push(ServiceWrapperResourcePair {
                wrapper: logical_name.clone(),
                resource: resource.clone(),
            });
        }
    }
    let response = store.register_wrapper_assets(&ServiceWrapperRegisterRequest { pairs })?;
    match response.rejected {
        None => {
            if !response.created_resources.is_empty() || !response.created_pairs.is_empty() {
                info!(
                    created_resources = response.created_resources.len(),
                    created_pairs = response.created_pairs.len(),
                    "backend auto-created wrapper assets on register",
                );
            }
        }
        Some(ServiceWrapperRegisterReject::AutoCreateDisabled {
            missing_resources,
            missing_pairs,
        }) => {
            warn!(
                missing_resources = missing_resources.len(),
                missing_pairs = missing_pairs.len(),
                "backend refused wrapper register: auto-create disabled for this client instance — \
                 an admin must enable `auto_create_wrapper_assets` on this instance, or create the \
                 missing resources and pairs manually",
            );
        }
    }
    Ok(())
}

fn build_enrollment_url(config: &DaemonConfig) -> String {
    format!("{}/enroll", config.backend.url.trim_end_matches('/'),)
}

pub(crate) fn unenrolled_message(url: &str) -> String {
    format!(
        "this underwrap instance is not enrolled yet. open the enrollment page in your browser: {url}"
    )
}

pub async fn run(config: DaemonConfig, shutdown: Option<Arc<Notify>>) -> Result<()> {
    // Validate config at startup: every wrapper.secret must reference a declared secret.
    validate_config(&config)?;

    // An instance is "unenrolled" when its client credential isn't on disk
    // yet. The NixOS module always sets `client_cert_file` / `client_key_file`
    // to per-instance paths, but those files only appear after the operator
    // runs `underwrap-keygen --instance-config /etc/underwrap/instances/<inst>.yaml`.
    // Missing-file → friendly enrollment guidance; populated → open the
    // backend client normally.
    let unenrolled = match (
        config.backend.client_cert_file.as_ref(),
        config.backend.client_key_file.as_ref(),
    ) {
        (Some(cert), Some(key)) => !cert.exists() || !key.exists(),
        _ => true,
    };
    let enrollment_url = if unenrolled {
        Some(build_enrollment_url(&config))
    } else {
        None
    };

    // Open the backend client when enrolled, and learn the
    // mTLS-bound client instance id by hitting `/v1/status` once at
    // startup. The id is used to look up grants in the per-store
    // policy document.
    let (store, client_instance): (Option<Arc<dyn StorageBackend>>, String) = if unenrolled {
        (None, String::new())
    } else {
        let store: Arc<dyn StorageBackend> =
            Arc::from(underwrap_store::open_backend_client(&config.backend)?);
        let principal = store
            .principal()
            .context("learning own runtime client instance id from backend /v1/status")?;
        info!(client_instance = %principal, "enrolled with backend");
        (Some(store), principal)
    };

    let wrapper_to_gid = build_wrapper_gid_map(&config)?;
    if let Some(store) = store.as_ref()
        && let Err(e) = register_wrapper_assets(store.as_ref(), &config)
    {
        warn!(err = %e, "wrapper asset registration failed; continuing without backend register");
    }

    let state = Arc::new(State {
        config: config.clone(),
        store,
        enrollment_url,
        client_instance,
        pending_approvals: PendingApprovals::new(),
    });

    let mut tasks = JoinSet::new();

    for (name, wrapper) in &config.wrappers {
        let gid = *wrapper_to_gid
            .get(name)
            .expect("wrapper gid must exist after startup validation");
        let listener = bind_socket(
            &wrapper.socket,
            0o660,
            &format!("wrapper '{name}'"),
            Some(gid),
        )?;
        tasks.spawn(accept_wrapper_loop(
            listener,
            Arc::clone(&state),
            name.clone(),
            gid,
        ));
    }

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
    wrapper_name: String,
    expected_gid: u32,
) -> Result<()> {
    let semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS));

    loop {
        let (stream, _addr) = listener.accept().await?;
        let state = Arc::clone(&state);
        let wrapper_name = wrapper_name.clone();
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore closed");

        tokio::spawn(async move {
            if let Err(e) = handle_wrapper(stream, state, &wrapper_name, expected_gid).await {
                error!(wrapper = %wrapper_name, "connection error: {e:#}");
            }
            drop(permit);
        });
    }
}
