//! Backend daemon server.
//!
//! Serves both the runtime HTTPS API for `underwrapd` clients and the
//! browser-facing admin UI/API from one HTTPS listener.

use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::collections::HashMap;

use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tracing::{error, info};

// External-consumer re-exports. `integration-tests/`, `tests/ts_export.rs`,
// and `main.rs` import these via `underwrap_backendd::server::*`. Once
// the crate root grows its own re-exports we can drop these — for now
// `server` remains the import path of record.
pub use crate::admin_api::auth::{LoginRequest, LoginResponse, WhoamiResponse};
pub use crate::admin_api::bootstrap::{
    BootstrapRequest, BootstrapResponse, BootstrapStatusResponse,
};
pub use crate::config::{
    AdminConfig, BackendConfig, DEFAULT_STORES_FILE, HttpsConfig, HttpsTlsConfig,
};
pub use crate::store_registry::{
    LoadedStoreRecord, RegistryLookupError, StoreRegistry, StoreState, is_valid_store_id,
    load_store_records,
};

// Internal helpers used by the runtime bootstrap and the HTTPS connection
// dispatcher.
use crate::http::origin::validate_admin_config;
use crate::http::responses::error_response;
use crate::runtime_api::handle_service_request;
use crate::runtime_api::principal::validate_runtime_client_mappings;
use crate::runtime_state::AdminRuntimeState;
use crate::store_registry::open_initial_stores;
use crate::tls::build_https_acceptor;

pub async fn run(config: BackendConfig, shutdown: Option<Arc<Notify>>) -> Result<()> {
    let stores_file = config.stores_file();
    let client_instances_file = config.client_instances_file();
    let users_file = config.users_file();
    let definitions = load_store_records(&stores_file)?;
    let state_map = open_initial_stores(&definitions)?;
    let store_registry = Arc::new(std::sync::RwLock::new(StoreRegistry {
        state: state_map,
        records: definitions.clone(),
    }));
    let client_instances = Arc::new(std::sync::RwLock::new(
        crate::runtime_clients::load_client_instances(&client_instances_file)?,
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
        let session_key = match admin_config {
            Some(admin) => crate::auth::load_session_key(&admin.session_key_file)?,
            None => [0u8; crate::auth::SESSION_KEY_LEN],
        };
        let state = Arc::new(AdminRuntimeState {
            stores: Arc::clone(&store_registry),
            client_instances: Arc::clone(&client_instances),
            stores_file: stores_file.clone(),
            client_instances_file: client_instances_file.clone(),
            stores_data_root: admin_config.and_then(|cfg| cfg.stores_data_root.clone()),
            rotation_locks: std::sync::Mutex::new(HashMap::new()),
            admin_enabled: admin_config.is_some(),
            allowed_origins: admin_config.and_then(|cfg| cfg.allowed_origins.clone()),
            web_source: crate::web::WebAssetSource::from_env(),
            users_file: users_file.clone(),
            session_key,
            session_lifetime_secs: admin_config
                .and_then(|cfg| cfg.session_lifetime_secs)
                .unwrap_or(crate::auth::DEFAULT_SESSION_LIFETIME_SECS),
            cookies_secure: true,
            login_rate_limiter: crate::auth::RateLimiter::new(),
            session_revocations: crate::auth::SessionRevocations::new(),
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

async fn https_accept_loop(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    state: Arc<AdminRuntimeState>,
) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_https_connection(stream, acceptor, state).await {
                error!("backend HTTPS connection error: {e:#}");
            }
        });
    }
}

async fn handle_https_connection(
    stream: tokio::net::TcpStream,
    acceptor: TlsAcceptor,
    state: Arc<AdminRuntimeState>,
) -> Result<()> {
    let tls_stream = acceptor
        .accept(stream)
        .await
        .context("accepting HTTPS connection")?;
    let peer_certificates = Arc::new(
        tls_stream
            .get_ref()
            .1
            .peer_certificates()
            .map(|certs| certs.to_vec())
            .unwrap_or_default(),
    );

    let service = service_fn(move |req| {
        let state = Arc::clone(&state);
        let peer_certificates = Arc::clone(&peer_certificates);
        async move { handle_https_request(req, state, peer_certificates).await }
    });

    hyper::server::conn::http1::Builder::new()
        .serve_connection(TokioIo::new(tls_stream), service)
        .await
        .context("serving HTTPS connection")?;
    Ok(())
}

async fn handle_https_request(
    request: Request<Incoming>,
    state: Arc<AdminRuntimeState>,
    peer_certificates: Arc<Vec<rustls::pki_types::CertificateDer<'static>>>,
) -> std::result::Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let path = request.uri().path();
    if path.starts_with("/v1/admin/") || crate::web::is_spa_request(request.method(), path) {
        if !state.admin_enabled {
            return Ok(error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                "admin interface is disabled",
            ));
        }
        return crate::admin_api::handle_admin_request(request, state).await;
    }
    handle_service_request(request, state, peer_certificates).await
}
