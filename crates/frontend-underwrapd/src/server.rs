//! Unix socket server for underwrapd.
//!
//! Runs wrapper socket listeners.

use anyhow::Result;
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinSet;
use tracing::{error, info, warn};

use underwrap_protocol::config::DaemonConfig;
use underwrap_protocol::service::CatalogEntry;

use crate::approval::PendingApprovals;
use crate::startup::{
    bind_socket, build_allowed_user_uids, build_wrapper_gid_map, next_listener_result,
    validate_config,
};
use crate::wrapper_flow::handle_wrapper;
use underwrap_store::StorageBackend;

/// Maximum concurrent connections per socket.
const MAX_CONNECTIONS: usize = 128;

/// Shared daemon state passed to connection handlers.
pub(crate) struct State {
    pub(crate) config: DaemonConfig,
    pub(crate) store: Arc<dyn StorageBackend>,
    pub(crate) allowed_user_uids: std::collections::HashSet<u32>,
    pub(crate) pending_approvals: PendingApprovals,
}

impl State {
    /// Get a clone of the storage backend Arc for sharing with channel handlers.
    pub(crate) fn store_arc(&self) -> Arc<dyn StorageBackend> {
        Arc::clone(&self.store)
    }
}

fn register_catalog(store: &dyn StorageBackend, config: &DaemonConfig) -> Result<()> {
    let users: Vec<String> = if config.allowed_users.is_empty() {
        vec!["*".to_string()]
    } else {
        config.allowed_users.clone()
    };

    let mut entries = Vec::new();
    for (wrapper_name, wrapper) in &config.wrappers {
        for resource in wrapper.resources.keys() {
            for user in &users {
                entries.push(CatalogEntry {
                    host: config.catalog_host.clone(),
                    user: user.clone(),
                    wrapper: wrapper_name.clone(),
                    resource: resource.clone(),
                });
            }
        }
    }
    store.register_catalog(&entries)
}

/// Run the daemon until `shutdown` is notified (or forever if `None`).
pub async fn run(config: DaemonConfig, shutdown: Option<Arc<Notify>>) -> Result<()> {
    // Validate config at startup: every wrapper.secret must reference a declared secret.
    validate_config(&config)?;

    // Open the backend client.
    let store: Arc<dyn StorageBackend> =
        Arc::from(underwrap_store::open_backend_client(&config.backend)?);

    let wrapper_to_gid = build_wrapper_gid_map(&config)?;
    let allowed_user_uids = build_allowed_user_uids(&config)?;
    if let Err(e) = register_catalog(&*store, &config) {
        warn!(err = %e, "catalog registration failed; continuing without backend catalog refresh");
    }

    let state = Arc::new(State {
        config: config.clone(),
        store,
        allowed_user_uids,
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
