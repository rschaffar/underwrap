//! Backend daemon server.
//!
//! Serves both the runtime HTTPS API for `underwrapd` clients and the
//! browser-facing admin UI/API from one HTTPS listener.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tracing::info;

use crate::config::BackendConfig;
use crate::http::https_accept_loop;
use crate::http::origin::validate_admin_config;
use crate::runtime_api::principal::validate_runtime_client_mappings;
use crate::runtime_state::AdminRuntimeState;
use crate::store_registry::{StoreRegistry, load_store_records, open_initial_stores};
use crate::tls::build_https_acceptor;

pub async fn run(config: BackendConfig, shutdown: Option<Arc<Notify>>) -> Result<()> {
    let stores_file = config.stores_file();
    let clients_dir = config.clients_dir();
    let users_file = config.users_file();
    let definitions = load_store_records(&stores_file)?;
    let state_map = open_initial_stores(&definitions)?;
    let store_registry = Arc::new(std::sync::RwLock::new(StoreRegistry {
        state: state_map,
        records: definitions.clone(),
    }));
    let client_instances = Arc::new(std::sync::RwLock::new(
        crate::runtime_clients::load_client_instances(&clients_dir)?,
    ));

    let mut tasks = JoinSet::new();

    if let Some(https_config) = &config.https {
        if let Some(admin_config) = &config.admin {
            validate_admin_config(admin_config)?;
        }
        validate_runtime_client_mappings(&store_registry, &client_instances)?;
        let listener = TcpListener::bind(&https_config.listen)
            .await
            .with_context(|| format!("binding HTTPS listener {}", https_config.listen))?;
        let tls_acceptor = build_https_acceptor(&https_config.tls)?;
        let admin_config = config.admin.as_ref();
        let state = Arc::new(AdminRuntimeState {
            stores: Arc::clone(&store_registry),
            client_instances: Arc::clone(&client_instances),
            stores_file: stores_file.clone(),
            clients_dir: clients_dir.clone(),
            stores_data_root: admin_config.and_then(|cfg| cfg.stores_data_root.clone()),
            rotation_locks: std::sync::Mutex::new(HashMap::new()),
            admin_enabled: admin_config.is_some(),
            allowed_origins: admin_config.and_then(|cfg| cfg.allowed_origins.clone()),
            users_file: users_file.clone(),
            auth_backend: crate::auth::AuthBackend::new(users_file.clone()),
            session_store: tower_sessions::MemoryStore::default(),
            csrf_config: crate::admin_api::csrf::build_csrf_config(true),
            session_lifetime_secs: admin_config
                .and_then(|cfg| cfg.session_lifetime_secs)
                .unwrap_or(crate::auth::DEFAULT_SESSION_LIFETIME_SECS),
            cookies_secure: true,
            bootstrap_lock: std::sync::Mutex::new(()),
            user_collection_lock: std::sync::Mutex::new(()),
        });
        {
            let snapshot = state.stores.read().unwrap();
            info!(
                listen = %https_config.listen,
                stores = snapshot.state.len(),
                instances = state.client_instances.read().unwrap().len(),
                admin = state.admin_enabled,
                "backend HTTPS listener ready",
            );
        }
        tasks.spawn(https_accept_loop(listener, tls_acceptor, state));
    } else if config.admin.is_some() {
        anyhow::bail!("backend config enables admin but has no https listener");
    }

    match shutdown {
        Some(notify) => {
            tokio::select! {
                r = tasks.join_next() => match r {
                    Some(result) => result.context("backend management task panicked")?,
                    None => Ok(()),
                },
                _ = notify.notified() => {
                    info!("backend shutdown signal received");
                    tasks.abort_all();
                    while tasks.join_next().await.is_some() {}
                    Ok(())
                }
            }
        }
        None => match tasks.join_next().await {
            Some(result) => result.context("backend management task panicked")?,
            None => Ok(()),
        },
    }
}
