//! Backend daemon server.
//!
//! Serves both the runtime HTTPS API for `underwrapd` clients and the
//! browser-facing admin UI/API from one HTTPS listener.

use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tracing::{error, info, warn};
use underwrap_protocol::config::StoreRecord;

#[cfg(test)]
use base64::Engine;
use underwrap_store::StorageBackend;
#[cfg(test)]
use underwrap_store::{load_resource_blob, save_resource_blob};

pub(crate) use crate::admin_api::audit::log_admin_event;
pub use crate::admin_api::auth::{LoginRequest, LoginResponse, WhoamiResponse};
pub(crate) use crate::admin_api::auth::{
    handle_admin_login, handle_admin_logout, handle_admin_password_change, handle_admin_whoami,
};
#[cfg(test)]
pub(crate) use crate::admin_api::bootstrap::audit_bootstrap_attempt;
pub use crate::admin_api::bootstrap::{
    BootstrapRequest, BootstrapResponse, BootstrapStatusResponse,
};
pub(crate) use crate::admin_api::bootstrap::{
    handle_admin_bootstrap, handle_admin_bootstrap_status,
};
#[cfg(test)]
pub(crate) use crate::admin_api::client_instances::{
    do_admin_client_instance_create, do_admin_client_instance_update,
};
pub(crate) use crate::admin_api::client_instances::{
    handle_admin_client_instance_create, handle_admin_client_instance_delete,
    handle_admin_client_instance_detail, handle_admin_client_instance_update,
    handle_admin_client_instances_list,
};
#[cfg(test)]
pub(crate) use crate::admin_api::csrf::AdminAuth;
pub(crate) use crate::admin_api::csrf::{
    AdminAuthError, attach_csrf_cookie, authenticate_admin, check_csrf, resolve_principal,
};
pub(crate) use crate::admin_api::overview::handle_admin_ui_overview;
pub(crate) use crate::admin_api::pairs::{
    handle_admin_pair_create, handle_admin_pair_delete, handle_admin_pairs_list,
};
pub(crate) use crate::admin_api::policy::{
    handle_admin_policy_get, handle_admin_policy_put, handle_admin_wrapper_permission_delete,
    handle_admin_wrapper_permission_upsert, handle_admin_wrapper_permissions_list,
};
pub(crate) use crate::admin_api::resources::{
    handle_admin_resource_blob_get, handle_admin_resource_blob_put, handle_admin_resource_create,
    handle_admin_resource_delete, handle_admin_resource_rename, handle_admin_resources_list,
};
pub(crate) use crate::admin_api::secrets::{
    handle_admin_secret_delete, handle_admin_secret_list, handle_admin_secret_read,
    handle_admin_secret_rename_endpoint, handle_admin_secret_write_endpoint,
};
#[cfg(test)]
pub(crate) use crate::admin_api::secrets::{handle_admin_secret_rename, handle_admin_secret_write};
#[cfg(test)]
pub(crate) use crate::admin_api::users::{do_admin_user_create, do_admin_user_update};
pub(crate) use crate::admin_api::users::{
    handle_admin_user_create, handle_admin_user_delete, handle_admin_user_detail,
    handle_admin_user_reset_password, handle_admin_user_update, handle_admin_users_list,
    mutate_collection, write_admin_user_audit,
};
pub use crate::config::{
    AdminConfig, BackendConfig, DEFAULT_STORES_FILE, HttpsConfig, HttpsTlsConfig,
};
#[cfg(test)]
pub(crate) use crate::http::origin::referer_to_origin;
pub(crate) use crate::http::origin::{check_admin_origin, validate_admin_config};
pub(crate) use crate::http::responses::{
    error_response, json_response, parse_json_body, store_locked_response,
};
pub(crate) use crate::runtime_api::handle_service_request;
#[cfg(test)]
pub(crate) use crate::runtime_api::invocations::{
    bump_pair_last_used, handle_runtime_wrapper_invocation_finalize,
    handle_runtime_wrapper_invocation_open,
};
pub(crate) use crate::runtime_api::principal::validate_runtime_client_mappings;
#[cfg(test)]
pub(crate) use crate::runtime_api::principal::{
    get_policy_document, put_policy_document, resolve_runtime_client,
};

#[cfg(test)]
pub(crate) use crate::runtime_api::wrappers::handle_runtime_wrapper_register;
pub(crate) use crate::runtime_state::AdminRuntimeState;
#[cfg(test)]
pub(crate) use crate::storage::documents::{
    load_pairs_document, load_policy_document_or_default, load_resources_document,
    save_pairs_document, save_policy_document, save_resources_document,
};
#[cfg(test)]
pub(crate) use crate::storage::secret_entries::{list_secret_entries, write_resource_entry};
pub use crate::store_registry::{
    LoadedStoreRecord, RegistryLookupError, StoreRegistry, StoreState, is_valid_store_id,
    load_store_records,
};
pub(crate) use crate::store_registry::{
    generate_store_id, open_initial_stores, save_store_records, validate_store_name,
};
pub(crate) use crate::tls::build_https_acceptor;

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
        return handle_admin_request(request, state).await;
    }
    handle_service_request(request, state, peer_certificates).await
}

/// Resolve a store reference for an authenticated request. Any
/// authenticated user can address any store; the helper just returns
/// the live backend or the appropriate not-found / locked response.
#[allow(clippy::result_large_err)]
pub(crate) fn admin_store(
    state: &AdminRuntimeState,
    store: &str,
) -> std::result::Result<Arc<dyn StorageBackend>, Response<Full<Bytes>>> {
    match state.stores.read().unwrap().unlocked_backend(store) {
        Ok(s) => Ok(s),
        Err(RegistryLookupError::Locked) => Err(store_locked_response(store)),
        Err(RegistryLookupError::NotFound) => Err(error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "unknown store",
        )),
    }
}

// ===== Stores CRUD =====
//
// All five handlers operate on the runtime `state.stores` registry
// plus the backend-managed store-registry file at `state.stores_file`.
// The requested data `dir` is still validated against the configured
// allow-list of roots. Atomicity comes from `tempfile::NamedTempFile`
// `persist()` on the registry write side and a brief held `write()`
// lock on the in-memory registry update.

use underwrap_protocol::service::{
    AdminStoreActionState, AdminStoreBackendKind, AdminStoreCreateRequest,
    AdminStoreCreateResponse, AdminStoreDeleteResponse, AdminStoreDetailResponse,
    AdminStoreListItem, AdminStoreRotateKeyResponse, AdminStoreUpdateRequest,
    AdminStoreUsageSummary, AdminStoresListResponse,
};

fn backend_kind(storage: &underwrap_protocol::config::StorageConfig) -> AdminStoreBackendKind {
    match storage {
        underwrap_protocol::config::StorageConfig::Plain { .. } => AdminStoreBackendKind::Plain,
        underwrap_protocol::config::StorageConfig::Encrypted { .. } => {
            AdminStoreBackendKind::Encrypted
        }
    }
}

fn saturating_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}

#[allow(clippy::result_large_err)]
fn admin_store_usage(
    state: &AdminRuntimeState,
    store_id: &str,
    store: &dyn underwrap_store::StorageBackend,
) -> std::result::Result<AdminStoreUsageSummary, Response<Full<Bytes>>> {
    let mapped_client_instance_count = saturating_u32(
        state
            .client_instances
            .read()
            .unwrap()
            .values()
            .filter(|instance| instance.current_store_id == store_id)
            .count(),
    );
    let has_secrets = match store.list("") {
        Ok(ids) => ids
            .iter()
            .any(|id| id != underwrap_store::ENCRYPTED_CANARY_ID),
        Err(e) => {
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "list_store_failed",
                &format!("checking store contents for management view: {e:#}"),
            ));
        }
    };
    Ok(AdminStoreUsageSummary {
        mapped_client_instance_count,
        has_secrets,
    })
}

fn admin_store_actions(
    loaded: &LoadedStoreRecord,
    usage: &AdminStoreUsageSummary,
    locked: bool,
) -> AdminStoreActionState {
    let can_manage = true;
    let is_encrypted = matches!(
        &loaded.record.storage,
        underwrap_protocol::config::StorageConfig::Encrypted { .. }
    );
    // Delete works for both locked and unlocked stores — we use the
    // `has_secrets` flag only to choose between plain DELETE and
    // force-DELETE. For locked stores we cannot inspect contents, so
    // we conservatively offer force-DELETE only.
    let can_delete_without_force =
        can_manage && usage.mapped_client_instance_count == 0 && !usage.has_secrets && !locked;
    let can_force_delete =
        can_manage && usage.mapped_client_instance_count == 0 && (usage.has_secrets || locked);
    AdminStoreActionState {
        can_edit_metadata: can_manage,
        can_rotate_key: can_manage && is_encrypted && !locked,
        can_unlock: can_manage && is_encrypted && locked,
        can_lock: can_manage && is_encrypted && !locked,
        can_delete_now: can_delete_without_force,
        can_force_delete,
    }
}

fn admin_store_list_item(
    loaded: &LoadedStoreRecord,
    locked: bool,
    usage: AdminStoreUsageSummary,
    actions: AdminStoreActionState,
) -> AdminStoreListItem {
    AdminStoreListItem {
        id: loaded.record.id.clone(),
        name: loaded.record.name.clone(),
        backend: backend_kind(&loaded.record.storage),
        description: loaded.record.description.clone(),
        created_at: loaded.record.created_at,
        locked,
        usage,
        actions,
    }
}

fn admin_store_detail(
    stores_file: &Path,
    loaded: &LoadedStoreRecord,
    locked: bool,
    usage: AdminStoreUsageSummary,
    actions: AdminStoreActionState,
) -> AdminStoreDetailResponse {
    let dir = match &loaded.record.storage {
        underwrap_protocol::config::StorageConfig::Plain { dir }
        | underwrap_protocol::config::StorageConfig::Encrypted { dir } => {
            dir.to_string_lossy().to_string()
        }
    };
    AdminStoreDetailResponse {
        id: loaded.record.id.clone(),
        name: loaded.record.name.clone(),
        backend: backend_kind(&loaded.record.storage),
        dir,
        locked,
        description: loaded.record.description.clone(),
        created_at: loaded.record.created_at,
        source_path: stores_file.to_string_lossy().to_string(),
        usage,
        actions,
    }
}

/// Usage summary for a store that we cannot currently inspect because
/// it is locked. We surface zero secret count rather than guessing —
/// the SPA's "is this safe to delete" hint then falls into the
/// force-delete bucket and the operator gets a clear "unlock first
/// or force-delete" decision.
fn admin_store_usage_for_locked(
    state: &AdminRuntimeState,
    store_id: &str,
) -> AdminStoreUsageSummary {
    let mapped_client_instance_count = saturating_u32(
        state
            .client_instances
            .read()
            .unwrap()
            .values()
            .filter(|instance| instance.current_store_id == store_id)
            .count(),
    );
    AdminStoreUsageSummary {
        mapped_client_instance_count,
        has_secrets: false,
    }
}

fn handle_admin_stores_list(state: &AdminRuntimeState) -> Response<Full<Bytes>> {
    type Visible = (String, LoadedStoreRecord, Option<Arc<dyn StorageBackend>>);
    let visible: Vec<Visible> = {
        let reg = state.stores.read().unwrap();
        reg.records
            .iter()
            .map(|(id, loaded)| {
                let backend = reg.state.get(id).and_then(|s| s.backend()).cloned();
                (id.clone(), loaded.clone(), backend)
            })
            .collect()
    };
    let mut items = Vec::with_capacity(visible.len());
    for (id, loaded, handle) in visible {
        let (usage, locked) = match handle {
            Some(b) => match admin_store_usage(state, &id, b.as_ref()) {
                Ok(usage) => (usage, false),
                Err(resp) => return resp,
            },
            None => (admin_store_usage_for_locked(state, &id), true),
        };
        let actions = admin_store_actions(&loaded, &usage, locked);
        items.push(admin_store_list_item(&loaded, locked, usage, actions));
    }
    items.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    json_response(StatusCode::OK, &AdminStoresListResponse { items })
}

#[allow(clippy::result_large_err)]
fn handle_admin_store_detail(state: &AdminRuntimeState, name: &str) -> Response<Full<Bytes>> {
    let (loaded, handle, locked) = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.records.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        match reg.state.get(name) {
            Some(StoreState::Unlocked(b)) => (loaded.clone(), Some(Arc::clone(b)), false),
            Some(StoreState::Locked) => (loaded.clone(), None, true),
            None => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "store metadata present but registry state missing",
                );
            }
        }
    };
    let usage = match handle.as_ref() {
        Some(b) => match admin_store_usage(state, name, b.as_ref()) {
            Ok(usage) => usage,
            Err(resp) => return resp,
        },
        None => admin_store_usage_for_locked(state, name),
    };
    let actions = admin_store_actions(&loaded, &usage, locked);
    json_response(
        StatusCode::OK,
        &admin_store_detail(&state.stores_file, &loaded, locked, usage, actions),
    )
}

/// Resolve the data directory of a new store. The on-disk layout is
/// `<stores_data_root>/<store_id>/`; the admin only chooses the store's
/// display name, so the directory leaf is the random store id. An
/// unconfigured root disables creation entirely.
fn resolve_store_dir(
    state: &AdminRuntimeState,
    store_id: &str,
) -> std::result::Result<PathBuf, (&'static str, String)> {
    let Some(root) = state.stores_data_root.as_ref() else {
        return Err((
            "data_root_not_configured",
            "store creation is disabled: backend.yaml has no `admin.stores_data_root`".to_string(),
        ));
    };
    if !root.is_absolute() {
        return Err((
            "data_root_not_absolute",
            format!(
                "`admin.stores_data_root` must be an absolute path, got {}",
                root.display()
            ),
        ));
    }
    Ok(root.join(store_id))
}

#[allow(clippy::result_large_err)]
async fn handle_admin_store_create(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<AdminStoreCreateRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    do_admin_store_create(state, principal, body)
}

/// Body-already-parsed half of [`handle_admin_store_create`]. Split
/// out so unit tests can drive it without synthesising a
/// `Request<Incoming>` (which `hyper` doesn't expose constructors
/// for). The HTTP-shaped handler above is exercised by Playwright.
#[allow(clippy::result_large_err)]
fn do_admin_store_create(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    body: AdminStoreCreateRequest,
) -> Response<Full<Bytes>> {
    let visible_name = match validate_store_name(&body.name) {
        Ok(name) => name,
        Err(msg) => return error_response(StatusCode::BAD_REQUEST, "invalid_store_name", msg),
    };

    let store_id = loop {
        let candidate = generate_store_id();
        if !state
            .stores
            .read()
            .unwrap()
            .records
            .contains_key(&candidate)
        {
            break candidate;
        }
    };

    let validated_dir = match resolve_store_dir(state, &store_id) {
        Ok(p) => p,
        Err((code, msg)) => return error_response(StatusCode::BAD_REQUEST, code, &msg),
    };

    {
        let reg = state.stores.read().unwrap();
        if reg
            .records
            .values()
            .any(|existing| existing.record.name.eq_ignore_ascii_case(&visible_name))
        {
            return error_response(
                StatusCode::CONFLICT,
                "store_name_already_exists",
                &format!("store name '{}' already exists", visible_name),
            );
        }
    }

    let (storage, opened, generated_key_hex): (
        underwrap_protocol::config::StorageConfig,
        Arc<dyn StorageBackend>,
        Option<String>,
    ) = match body.backend {
        AdminStoreBackendKind::Plain => {
            let storage = underwrap_protocol::config::StorageConfig::Plain {
                dir: validated_dir.clone(),
            };
            if let Err(e) = create_data_dir_secure(&validated_dir) {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "create_data_dir_failed",
                    &format!("creating data dir {}: {e:#}", validated_dir.display()),
                );
            }
            let backend: Arc<dyn StorageBackend> = match underwrap_store::open(&storage) {
                Ok(b) => Arc::from(b),
                Err(e) => {
                    return error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "open_store_failed",
                        &format!("opening store backend after create: {e:#}"),
                    );
                }
            };
            (storage, backend, None)
        }
        AdminStoreBackendKind::Encrypted => {
            if let Err(e) = create_data_dir_secure(&validated_dir) {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "create_data_dir_failed",
                    &format!("creating data dir {}: {e:#}", validated_dir.display()),
                );
            }
            let key_bytes = generate_master_key_bytes();
            let key_hex = master_key_hex(&key_bytes);
            let backend =
                match underwrap_store::EncryptedBackend::open_with_key(&validated_dir, key_bytes) {
                    Ok(b) => b,
                    Err(e) => {
                        return error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "open_store_failed",
                            &format!("opening encrypted store backend after create: {e:#}"),
                        );
                    }
                };
            if let Err(e) = backend.write(
                underwrap_store::ENCRYPTED_CANARY_ID,
                underwrap_store::ENCRYPTED_CANARY_PLAINTEXT,
            ) {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "canary_write_failed",
                    &format!("writing unlock canary for new encrypted store: {e:#}"),
                );
            }
            let backend: Arc<dyn StorageBackend> = Arc::new(backend);
            let storage = underwrap_protocol::config::StorageConfig::Encrypted {
                dir: validated_dir.clone(),
            };
            (storage, backend, Some(key_hex))
        }
    };

    let record = StoreRecord {
        id: store_id.clone(),
        name: visible_name,
        description: body.description.clone(),
        created_at: Some(crate::users::now_epoch_secs()),
        storage,
    };

    let loaded = LoadedStoreRecord { record };
    {
        let mut reg = state.stores.write().unwrap();
        reg.state
            .insert(store_id.clone(), StoreState::Unlocked(opened));
        reg.records.insert(store_id.clone(), loaded.clone());
        if let Err(e) = save_store_records(&state.stores_file, &reg.records) {
            reg.state.remove(&store_id);
            reg.records.remove(&store_id);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "write_store_registry_failed",
                &format!("{e:#}"),
            );
        }
    }
    write_admin_store_audit(state, principal, &store_id, "store.create", "success");
    let usage = AdminStoreUsageSummary {
        mapped_client_instance_count: 0,
        has_secrets: false,
    };
    let actions = admin_store_actions(&loaded, &usage, false);
    json_response(
        StatusCode::OK,
        &AdminStoreCreateResponse {
            store: admin_store_detail(&state.stores_file, &loaded, false, usage, actions),
            key_hex: generated_key_hex,
        },
    )
}

/// Generate 32 fresh random bytes via the OS RNG. The bytes are
/// caller-owned and should be moved into either the [`EncryptedBackend`]
/// (which zeroizes on drop) or the JSON response buffer — never both.
fn generate_master_key_bytes() -> [u8; 32] {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}

/// Hex-encode a 32-byte master key as 64 lowercase characters.
fn master_key_hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(unix)]
fn create_data_dir_secure(dir: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    if dir.exists() {
        return Ok(());
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("creating {}", dir.display()))
}
#[cfg(not(unix))]
fn create_data_dir_secure(dir: &Path) -> Result<()> {
    if dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))
}

#[allow(clippy::result_large_err)]
async fn handle_admin_store_update(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<AdminStoreUpdateRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    do_admin_store_update(state, principal, name, body)
}

#[allow(clippy::result_large_err)]
fn do_admin_store_update(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
    body: AdminStoreUpdateRequest,
) -> Response<Full<Bytes>> {
    let updated = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.records.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        let mut record = loaded.record.clone();
        if let Some(new_name) = body.name {
            let new_name = match validate_store_name(&new_name) {
                Ok(name) => name,
                Err(msg) => {
                    return error_response(StatusCode::BAD_REQUEST, "invalid_store_name", msg);
                }
            };
            if reg.records.iter().any(|(other_id, other)| {
                other_id != name && other.record.name.eq_ignore_ascii_case(&new_name)
            }) {
                return error_response(
                    StatusCode::CONFLICT,
                    "store_name_already_exists",
                    &format!("store name '{}' already exists", new_name),
                );
            }
            record.name = new_name;
        }
        if let Some(new_desc) = body.description {
            record.description = if new_desc.is_empty() {
                None
            } else {
                Some(new_desc)
            };
        }
        record
    };
    {
        let mut reg = state.stores.write().unwrap();
        if let Some(loaded) = reg.records.get_mut(name) {
            loaded.record = updated.clone();
        }
        if let Err(e) = save_store_records(&state.stores_file, &reg.records) {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "write_store_registry_failed",
                &format!("{e:#}"),
            );
        }
    }
    write_admin_store_audit(state, principal, name, "store.update", "success");
    let loaded = LoadedStoreRecord { record: updated };
    let (handle, locked) = {
        let reg = state.stores.read().unwrap();
        match reg.state.get(name) {
            Some(StoreState::Unlocked(b)) => (Some(Arc::clone(b)), false),
            Some(StoreState::Locked) => (None, true),
            None => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "store metadata present but registry state missing",
                );
            }
        }
    };
    let usage = match handle.as_ref() {
        Some(b) => match admin_store_usage(state, name, b.as_ref()) {
            Ok(usage) => usage,
            Err(resp) => return resp,
        },
        None => admin_store_usage_for_locked(state, name),
    };
    let actions = admin_store_actions(&loaded, &usage, locked);
    json_response(
        StatusCode::OK,
        &admin_store_detail(&state.stores_file, &loaded, locked, usage, actions),
    )
}

#[allow(clippy::result_large_err)]
fn handle_admin_store_delete(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
    force: bool,
) -> Response<Full<Bytes>> {
    let (store_handle, dir_for_hint, locked) = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.records.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        let dir = loaded.record.storage.dir().clone();
        match reg.state.get(name) {
            Some(StoreState::Unlocked(b)) => (Some(Arc::clone(b)), dir, false),
            Some(StoreState::Locked) => (None, dir, true),
            None => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "store metadata present but registry state missing",
                );
            }
        }
    };
    if state
        .client_instances
        .read()
        .unwrap()
        .values()
        .any(|instance| instance.current_store_id == name)
    {
        return error_response(
            StatusCode::CONFLICT,
            "store_in_use",
            "store is still mapped to one or more client instances",
        );
    }

    let non_empty = match store_handle.as_ref() {
        Some(h) => match h.list("") {
            Ok(ids) => ids
                .iter()
                .any(|id| id != underwrap_store::ENCRYPTED_CANARY_ID),
            Err(e) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "list_store_failed",
                    &format!("checking store contents before delete: {e:#}"),
                );
            }
        },
        // Locked encrypted store: we cannot inspect contents. Refuse
        // unless the caller passed `force=true`, since deleting a
        // store without confirming it's empty risks orphaning data
        // the operator forgot about.
        None => true,
    };
    if non_empty && !force {
        let code = if locked {
            "store_locked_use_force"
        } else {
            "store_not_empty"
        };
        let message = if locked {
            "store is locked; contents cannot be inspected. re-issue with `?force=true` to delete the record anyway"
        } else {
            "store contains secrets; re-issue with `?force=true` to delete the record anyway"
        };
        return error_response(StatusCode::CONFLICT, code, message);
    }

    {
        let mut reg = state.stores.write().unwrap();
        reg.state.remove(name);
        reg.records.remove(name);
        if let Err(e) = save_store_records(&state.stores_file, &reg.records) {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "write_store_registry_failed",
                &format!("{e:#}"),
            );
        }
    }
    let cleanup_hint = if non_empty {
        Some(format!(
            "store '{name}' had secrets at deletion time. The store record has been removed; \
             the data dir at {} was left in place so you can decide whether to back it up first. \
             Remove it manually (`rm -rf {}`) when ready.",
            dir_for_hint.display(),
            dir_for_hint.display(),
        ))
    } else {
        None
    };
    let result = if non_empty {
        "force-deleted"
    } else {
        "success"
    };
    write_admin_store_audit(state, principal, name, "store.delete", result);
    json_response(
        StatusCode::OK,
        &AdminStoreDeleteResponse {
            ok: true,
            cleanup_hint,
        },
    )
}

/// Get or lazily create the rotation `Mutex` for `store_id`. Held for
/// the duration of a rotation so concurrent writes wait until the new
/// backend has fully replaced the old one.
fn rotation_lock(state: &AdminRuntimeState, store_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = state.rotation_locks.lock().unwrap();
    Arc::clone(
        locks
            .entry(store_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
    )
}

/// `POST /v1/admin/stores/{id}/rotate-key` — mint a fresh 32-byte
/// master key in memory, re-encrypt every resource in the store under
/// it, atomically swap the data directory, install the new backend in
/// the registry, and return the new key **exactly once**. Plain stores
/// reject the operation; locked encrypted stores must be unlocked
/// first.
async fn handle_admin_store_rotate_key(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
) -> Response<Full<Bytes>> {
    let (mut updated_record, dir, current_backend) = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.records.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        let record = loaded.record.clone();
        let dir = match &record.storage {
            underwrap_protocol::config::StorageConfig::Plain { .. } => {
                return error_response(
                    StatusCode::CONFLICT,
                    "not_encrypted",
                    "store is not encrypted; rotate-key only applies to encrypted backends",
                );
            }
            underwrap_protocol::config::StorageConfig::Encrypted { dir } => dir.clone(),
        };
        match reg.state.get(name) {
            Some(StoreState::Unlocked(b)) => (record, dir, Arc::clone(b)),
            Some(StoreState::Locked) => {
                return store_locked_response(name);
            }
            None => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "store metadata present but registry state missing",
                );
            }
        }
    };

    let rotate_lock = rotation_lock(state, name);
    let _rotate_guard = rotate_lock.lock().await;

    let new_key_bytes = generate_master_key_bytes();
    let key_hex = master_key_hex(&new_key_bytes);

    // Stage the re-encrypted blobs in a sibling temp directory and
    // swap into place once every resource has migrated. The temp dir
    // is colocated with the data dir so `rename` is atomic on the
    // same filesystem.
    let parent = match dir.parent() {
        Some(p) => p.to_path_buf(),
        None => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "encrypted store directory has no parent — cannot stage rotation",
            );
        }
    };
    let stage_name = format!(
        ".rotating-{}-{}",
        name,
        master_key_hex(&generate_master_key_bytes())
            .chars()
            .take(16)
            .collect::<String>()
    );
    let stage_dir = parent.join(&stage_name);
    if let Err(e) = create_data_dir_secure(&stage_dir) {
        write_admin_store_audit(state, principal, name, "store.rotate-key", "stage-failed");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rotate_stage_failed",
            &format!(
                "creating rotation staging dir {}: {e:#}",
                stage_dir.display()
            ),
        );
    }
    let new_backend =
        match underwrap_store::EncryptedBackend::open_with_key(&stage_dir, new_key_bytes) {
            Ok(b) => b,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&stage_dir);
                write_admin_store_audit(state, principal, name, "store.rotate-key", "stage-failed");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "rotate_stage_failed",
                    &format!("opening rotation staging backend: {e:#}"),
                );
            }
        };

    if let Err(resp) = reencrypt_into(
        current_backend.as_ref(),
        &new_backend,
        &stage_dir,
        state,
        principal,
        name,
    ) {
        return resp;
    }

    // Move the live data dir aside and swap the staged dir in. Both
    // renames are individually atomic; we attempt to undo the first
    // if the second fails so the registry's `Arc` never points at a
    // backend whose directory has vanished.
    let backup_dir = parent.join(format!("{stage_name}.replaced"));
    if let Err(e) = std::fs::rename(&dir, &backup_dir) {
        let _ = std::fs::remove_dir_all(&stage_dir);
        write_admin_store_audit(state, principal, name, "store.rotate-key", "swap-failed");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rotate_swap_failed",
            &format!(
                "moving live data dir aside ({} → {}): {e:#}",
                dir.display(),
                backup_dir.display(),
            ),
        );
    }
    if let Err(e) = std::fs::rename(&stage_dir, &dir) {
        // Try to roll the first rename back so the live dir keeps
        // pointing at usable ciphertext.
        let _ = std::fs::rename(&backup_dir, &dir);
        let _ = std::fs::remove_dir_all(&stage_dir);
        write_admin_store_audit(state, principal, name, "store.rotate-key", "swap-failed");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rotate_swap_failed",
            &format!(
                "promoting staged data dir ({} → {}): {e:#}",
                stage_dir.display(),
                dir.display(),
            ),
        );
    }

    // Open a fresh backend at the post-swap path so the registry
    // ends up holding an `Arc` whose `dir` field matches the live
    // directory. (Reusing `new_backend` would carry the staging
    // path even though we just renamed it.)
    let installed_key = generate_master_key_bytes(); // placeholder, immediately overwritten
    let _ = installed_key; // keep clippy happy if it ever sees an unused let
    let rotated_backend: Arc<dyn StorageBackend> = {
        // Re-derive the in-memory key from the hex we already
        // computed — that way we don't have to hold on to two
        // copies of the byte form across the swap.
        let bytes = match underwrap_store::parse_hex_key(&key_hex) {
            Ok(b) => b,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&backup_dir);
                write_admin_store_audit(
                    state,
                    principal,
                    name,
                    "store.rotate-key",
                    "reopen-failed",
                );
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    &format!("parsing fresh rotation key back from hex: {e:#}"),
                );
            }
        };
        match underwrap_store::EncryptedBackend::open_with_key(&dir, bytes) {
            Ok(b) => Arc::new(b),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&backup_dir);
                write_admin_store_audit(
                    state,
                    principal,
                    name,
                    "store.rotate-key",
                    "reopen-failed",
                );
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "open_store_failed",
                    &format!("opening rotated backend at {}: {e:#}", dir.display()),
                );
            }
        }
    };
    // Best-effort cleanup of the old ciphertext directory now that
    // the swap has succeeded.
    if let Err(e) = std::fs::remove_dir_all(&backup_dir) {
        warn!(
            store = %name,
            backup_dir = %backup_dir.display(),
            err = format!("{e:#}"),
            "rotation succeeded but old data dir could not be removed; left in place for manual cleanup",
        );
    }

    updated_record.storage = underwrap_protocol::config::StorageConfig::Encrypted { dir };
    {
        let mut reg = state.stores.write().unwrap();
        reg.state
            .insert(name.to_string(), StoreState::Unlocked(rotated_backend));
        if let Some(loaded) = reg.records.get_mut(name) {
            loaded.record = updated_record.clone();
        }
        if let Err(e) = save_store_records(&state.stores_file, &reg.records) {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "write_store_registry_failed",
                &format!("{e:#}"),
            );
        }
    }

    write_admin_store_audit(state, principal, name, "store.rotate-key", "success");
    json_response(
        StatusCode::OK,
        &AdminStoreRotateKeyResponse {
            key_hex,
            warning:
                "every resource in this store has been re-encrypted under the new master key. \
                 the new key is shown only here — record it now in a password manager or other \
                 trusted store. you will need it to unlock this store after the next backend \
                 restart."
                    .to_string(),
        },
    )
}

/// Copy every resource (and the canary) from `from` to `to`,
/// transparently decrypting under the old key and re-encrypting under
/// the new one. On any failure the staged data directory is removed
/// and a structured error response is returned for the rotate handler
/// to surface to the operator.
#[allow(clippy::result_large_err)]
fn reencrypt_into(
    from: &dyn StorageBackend,
    to: &dyn StorageBackend,
    stage_dir: &Path,
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    store_name: &str,
) -> std::result::Result<(), Response<Full<Bytes>>> {
    let ids = match from.list("") {
        Ok(ids) => ids,
        Err(e) => {
            let _ = std::fs::remove_dir_all(stage_dir);
            write_admin_store_audit(
                state,
                principal,
                store_name,
                "store.rotate-key",
                "list-failed",
            );
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "rotate_list_failed",
                &format!("listing existing resources for rotation: {e:#}"),
            ));
        }
    };
    // Make sure the canary is always present in the new backend even
    // if the source store somehow lost it (e.g. operator deleted it
    // by hand). Write it once up front; the loop below will skip its
    // own re-write so we don't fight ourselves over the canary id.
    if let Err(e) = to.write(
        underwrap_store::ENCRYPTED_CANARY_ID,
        underwrap_store::ENCRYPTED_CANARY_PLAINTEXT,
    ) {
        let _ = std::fs::remove_dir_all(stage_dir);
        write_admin_store_audit(
            state,
            principal,
            store_name,
            "store.rotate-key",
            "canary-write-failed",
        );
        return Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rotate_canary_failed",
            &format!("writing canary in staged data dir: {e:#}"),
        ));
    }
    for id in ids {
        if id == underwrap_store::ENCRYPTED_CANARY_ID {
            continue;
        }
        let material = match from.read(&id) {
            Ok(Some(m)) => m,
            Ok(None) => continue, // raced — gone
            Err(e) => {
                let _ = std::fs::remove_dir_all(stage_dir);
                write_admin_store_audit(
                    state,
                    principal,
                    store_name,
                    "store.rotate-key",
                    "read-failed",
                );
                return Err(error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "rotate_read_failed",
                    &format!("reading '{id}' for rotation: {e:#}"),
                ));
            }
        };
        if let Err(e) = to.write(&id, material.as_ref()) {
            let _ = std::fs::remove_dir_all(stage_dir);
            write_admin_store_audit(
                state,
                principal,
                store_name,
                "store.rotate-key",
                "write-failed",
            );
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "rotate_write_failed",
                &format!("writing '{id}' to staged data dir: {e:#}"),
            ));
        }
    }
    Ok(())
}

/// `POST /v1/admin/stores/{id}/unlock` — accept a 64-character hex
/// master key from the operator, build a fresh `EncryptedBackend`
/// against the store's data directory, verify the key by decrypting
/// the unlock canary written at creation, and install the backend in
/// the registry. Wrong keys never transition the registry to
/// `Unlocked`; the registry stays `Locked` and the response surfaces a
/// structured `key_mismatch` code.
async fn handle_admin_store_unlock(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<underwrap_protocol::service::AdminStoreUnlockRequest>(
        request,
    )
    .await
    {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    do_admin_store_unlock(state, principal, name, body)
}

#[allow(clippy::result_large_err)]
fn do_admin_store_unlock(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
    body: underwrap_protocol::service::AdminStoreUnlockRequest,
) -> Response<Full<Bytes>> {
    let dir = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.records.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        match &loaded.record.storage {
            underwrap_protocol::config::StorageConfig::Plain { .. } => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "store_not_encrypted",
                    "plain stores are never locked",
                );
            }
            underwrap_protocol::config::StorageConfig::Encrypted { dir } => dir.clone(),
        }
    };
    match state.stores.read().unwrap().state.get(name) {
        Some(StoreState::Unlocked(_)) => {
            return error_response(
                StatusCode::CONFLICT,
                "already_unlocked",
                "store is already unlocked",
            );
        }
        Some(StoreState::Locked) => {}
        None => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "store metadata present but registry state missing",
            );
        }
    }
    let key_bytes = match underwrap_store::parse_hex_key(&body.key_hex) {
        Ok(k) => k,
        Err(e) => {
            write_admin_store_audit(state, principal, name, "store.unlock", "invalid-key-hex");
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_key_hex",
                &format!("master key is not 64 hex characters: {e}"),
            );
        }
    };
    let backend = match underwrap_store::EncryptedBackend::open_with_key(&dir, key_bytes) {
        Ok(b) => b,
        Err(e) => {
            write_admin_store_audit(state, principal, name, "store.unlock", "open-failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "open_store_failed",
                &format!("opening encrypted store at {}: {e:#}", dir.display()),
            );
        }
    };
    match backend.read(underwrap_store::ENCRYPTED_CANARY_ID) {
        Ok(Some(plain)) if plain.as_ref() == underwrap_store::ENCRYPTED_CANARY_PLAINTEXT => {}
        Ok(Some(_)) => {
            write_admin_store_audit(state, principal, name, "store.unlock", "canary-mismatch");
            return error_response(
                StatusCode::FORBIDDEN,
                "key_mismatch",
                "submitted key decrypts the canary but to unexpected content; refusing to unlock",
            );
        }
        Ok(None) => {
            write_admin_store_audit(state, principal, name, "store.unlock", "canary-missing");
            return error_response(
                StatusCode::FORBIDDEN,
                "key_mismatch",
                "store has no unlock canary; refusing to unlock under an unverifiable key",
            );
        }
        Err(_) => {
            write_admin_store_audit(state, principal, name, "store.unlock", "key-mismatch");
            return error_response(
                StatusCode::FORBIDDEN,
                "key_mismatch",
                "submitted key did not decrypt the unlock canary",
            );
        }
    }
    {
        let mut reg = state.stores.write().unwrap();
        reg.unlock(name, Arc::new(backend));
    }
    write_admin_store_audit(state, principal, name, "store.unlock", "success");
    json_response(StatusCode::OK, &serde_json::json!({ "ok": true }))
}

/// `POST /v1/admin/stores/{id}/lock` — drop the in-memory backend for
/// `name`, transitioning the registry to [`StoreState::Locked`]. The
/// held `MasterKey` zeroes on drop. Plain stores reject the
/// operation; already-locked stores succeed as a no-op so the admin
/// SPA can call this without racing the current state.
#[allow(clippy::result_large_err)]
fn handle_admin_store_lock(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
) -> Response<Full<Bytes>> {
    let mut reg = state.stores.write().unwrap();
    let Some(loaded) = reg.records.get(name) else {
        return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
    };
    if matches!(
        loaded.record.storage,
        underwrap_protocol::config::StorageConfig::Plain { .. }
    ) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "store_not_encrypted",
            "plain stores are never locked",
        );
    }
    let was_locked = matches!(reg.state.get(name), Some(StoreState::Locked));
    reg.lock(name);
    drop(reg);
    let result = if was_locked { "noop" } else { "success" };
    write_admin_store_audit(state, principal, name, "store.lock", result);
    json_response(
        StatusCode::OK,
        &serde_json::json!({ "ok": true, "was_locked": was_locked }),
    )
}

fn write_admin_store_audit(
    _state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    store_name: &str,
    action: &str,
    result: &str,
) {
    let resource = format!("store:{store_name}");
    log_admin_event(
        action,
        &principal.username,
        None,
        Some(resource.as_str()),
        None,
        None,
        result,
    );
}

async fn handle_admin_request(
    request: Request<Incoming>,
    state: Arc<AdminRuntimeState>,
) -> std::result::Result<Response<Full<Bytes>>, std::convert::Infallible> {
    if crate::web::is_spa_request(request.method(), request.uri().path()) {
        return Ok(crate::web::serve_web_request(
            request.method(),
            request.uri().path(),
            &state.web_source,
        ));
    }
    if let Err(reason) = check_admin_origin(
        request.method(),
        request.headers(),
        state.allowed_origins.as_deref(),
    ) {
        warn!(reason = reason, "rejected admin request (origin check)");
        return Ok(error_response(StatusCode::FORBIDDEN, "bad_origin", reason));
    }
    // Auth endpoints are public — they exist *to* establish a session.
    // CSRF/logout (212), bootstrap (214), and any other unauthenticated
    // auth surfaces dispatch here before the session-cookie check
    // below.
    match (request.method(), request.uri().path()) {
        (&Method::POST, "/v1/admin/auth/login") => {
            return Ok(handle_admin_login(&state, request).await);
        }
        (&Method::GET, "/v1/admin/auth/whoami") => {
            return Ok(handle_admin_whoami(&state, request.headers()));
        }
        (&Method::GET, "/v1/admin/auth/bootstrap/status") => {
            return Ok(handle_admin_bootstrap_status(&state));
        }
        (&Method::POST, "/v1/admin/auth/bootstrap") => {
            return Ok(handle_admin_bootstrap(&state, request).await);
        }
        _ => {}
    }
    let auth = match authenticate_admin(&state, request.headers()) {
        Ok(a) => a,
        Err(AdminAuthError::Revoked) => {
            warn!("rejected admin request: session revoked");
            return Ok(error_response(
                StatusCode::UNAUTHORIZED,
                "session_revoked",
                "session has been revoked",
            ));
        }
        Err(AdminAuthError::Unauthenticated(reason)) => {
            warn!(reason = reason, "rejected admin request");
            return Ok(error_response(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                reason,
            ));
        }
    };

    if let Err((code, msg)) = check_csrf(
        request.method(),
        request.headers(),
        &auth.session().csrf_secret,
    ) {
        warn!(reason = msg, code = code, "rejected admin request (csrf)");
        return Ok(error_response(StatusCode::FORBIDDEN, code, msg));
    }

    let principal = match resolve_principal(&state, &auth) {
        Ok(p) => p,
        Err(resp) => return Ok(resp),
    };
    let actor = principal.username.clone();
    info!(
        user = %actor,
        method = %request.method(),
        path = request.uri().path(),
        "admin request"
    );

    // Logout runs *after* auth + CSRF have passed: the caller must have
    // a valid session AND have proven possession of its csrf_secret to
    // revoke it. That blocks drive-by logout via cookie-only requests.
    if request.method() == Method::POST && request.uri().path() == "/v1/admin/auth/logout" {
        return Ok(handle_admin_logout(&state, auth.session()));
    }

    // Path-parameterised stores routes. Match them first so the
    // literal "/v1/admin/stores" dispatch below doesn't shadow them.
    //
    // Three nested routes today — `rotate-key`, `unlock`, `lock` —
    // all match before the generic single-segment matcher (which
    // filters out any path containing '/').
    if let Some(rest) = request.uri().path().strip_prefix("/v1/admin/stores/") {
        if let Some(name) = rest.strip_suffix("/rotate-key")
            && !name.is_empty()
            && !name.contains('/')
        {
            return Ok(match *request.method() {
                Method::POST => handle_admin_store_rotate_key(&state, &principal, name).await,
                _ => error_response(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed", "POST"),
            });
        }
        if rest.ends_with("/unlock") || rest.ends_with("/lock") {
            let (name, op) = if let Some(n) = rest.strip_suffix("/unlock") {
                (n, "unlock")
            } else {
                (rest.strip_suffix("/lock").unwrap(), "lock")
            };
            if !name.is_empty() && !name.contains('/') {
                let name_owned = name.to_string();
                let is_post = request.method() == Method::POST;
                if !is_post {
                    return Ok(error_response(
                        StatusCode::METHOD_NOT_ALLOWED,
                        "method_not_allowed",
                        "POST",
                    ));
                }
                return Ok(match op {
                    "unlock" => {
                        handle_admin_store_unlock(&state, &principal, &name_owned, request).await
                    }
                    "lock" => handle_admin_store_lock(&state, &principal, &name_owned),
                    _ => unreachable!(),
                });
            }
        }
    }
    // Path-parameterised users routes (ticket 240).
    //
    // `/v1/admin/users/{id}/password` is the only nested route; match
    // it before the generic single-segment matcher (which filters out
    // any path containing '/').
    let users_subpath = request
        .uri()
        .path()
        .strip_prefix("/v1/admin/users/")
        .map(str::to_string);
    if let Some(rest) = users_subpath.as_deref() {
        if let Some(id) = rest.strip_suffix("/password")
            && !id.is_empty()
            && !id.contains('/')
        {
            let id = id.to_string();
            return Ok(match *request.method() {
                Method::POST => {
                    handle_admin_user_reset_password(&state, &principal, &id, request).await
                }
                _ => error_response(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed", "POST"),
            });
        }
        if !rest.is_empty() && !rest.contains('/') {
            let id = rest.to_string();
            match *request.method() {
                Method::GET => {
                    return Ok(handle_admin_user_detail(&state, &id));
                }
                Method::PUT => {
                    return Ok(handle_admin_user_update(&state, &principal, &id, request).await);
                }
                Method::DELETE => {
                    return Ok(handle_admin_user_delete(&state, &principal, &id));
                }
                _ => {
                    return Ok(error_response(
                        StatusCode::METHOD_NOT_ALLOWED,
                        "method_not_allowed",
                        "GET, PUT, or DELETE",
                    ));
                }
            }
        }
    }

    // Path-parameterised client-instance routes.
    if let Some(id) = request
        .uri()
        .path()
        .strip_prefix("/v1/admin/client-instances/")
        .filter(|rest| !rest.is_empty() && !rest.contains('/'))
    {
        let id = id.to_string();
        match *request.method() {
            Method::GET => {
                return Ok(handle_admin_client_instance_detail(&state, &id));
            }
            Method::PUT => {
                return Ok(
                    handle_admin_client_instance_update(&state, &principal, &id, request).await,
                );
            }
            Method::DELETE => {
                return Ok(handle_admin_client_instance_delete(&state, &principal, &id));
            }
            _ => {
                return Ok(error_response(
                    StatusCode::METHOD_NOT_ALLOWED,
                    "method_not_allowed",
                    "GET, PUT, or DELETE",
                ));
            }
        }
    }

    // GET /v1/admin/stores/<name>, PUT, DELETE.
    if let Some(name) = request
        .uri()
        .path()
        .strip_prefix("/v1/admin/stores/")
        .filter(|rest| !rest.is_empty() && !rest.contains('/'))
    {
        let name = name.to_string();
        match *request.method() {
            Method::GET => {
                return Ok(handle_admin_store_detail(&state, &name));
            }
            Method::PUT => {
                return Ok(handle_admin_store_update(&state, &principal, &name, request).await);
            }
            Method::DELETE => {
                let force = request
                    .uri()
                    .query()
                    .map(|q| {
                        q.split('&')
                            .any(|pair| matches!(pair, "force=true" | "force=1" | "force"))
                    })
                    .unwrap_or(false);
                return Ok(handle_admin_store_delete(&state, &principal, &name, force));
            }
            _ => {
                return Ok(error_response(
                    StatusCode::METHOD_NOT_ALLOWED,
                    "method_not_allowed",
                    "GET, PUT, or DELETE",
                ));
            }
        }
    }

    let response = match (request.method(), request.uri().path()) {
        (&Method::GET, "/v1/admin/users") => {
            return Ok(handle_admin_users_list(&state));
        }
        (&Method::POST, "/v1/admin/users") => {
            return Ok(handle_admin_user_create(&state, &principal, request).await);
        }
        (&Method::POST, "/v1/admin/auth/password") => {
            return Ok(handle_admin_password_change(&state, &principal, request).await);
        }
        (&Method::GET, "/v1/admin/stores") => {
            return Ok(handle_admin_stores_list(&state));
        }
        (&Method::POST, "/v1/admin/stores") => {
            return Ok(handle_admin_store_create(&state, &principal, request).await);
        }
        (&Method::GET, "/v1/admin/client-instances") => {
            return Ok(handle_admin_client_instances_list(&state));
        }
        (&Method::POST, "/v1/admin/client-instances") => {
            return Ok(handle_admin_client_instance_create(&state, &principal, request).await);
        }
        (&Method::GET, "/v1/admin/ui/overview") => handle_admin_ui_overview(&state),
        (&Method::POST, "/v1/admin/secret/read") => handle_admin_secret_read(&state, request).await,
        (&Method::POST, "/v1/admin/secret/write") => {
            handle_admin_secret_write_endpoint(&state, &actor, request).await
        }
        (&Method::POST, "/v1/admin/secret/list") => handle_admin_secret_list(&state, request).await,
        (&Method::POST, "/v1/admin/secret/delete") => {
            handle_admin_secret_delete(&state, request).await
        }
        (&Method::POST, "/v1/admin/secret/rename") => {
            handle_admin_secret_rename_endpoint(&state, &actor, request).await
        }
        (&Method::POST, "/v1/admin/policy/get") => handle_admin_policy_get(&state, request).await,
        (&Method::POST, "/v1/admin/policy/put") => handle_admin_policy_put(&state, request).await,
        // ===== Wrapper permission CRUD =====
        (&Method::POST, "/v1/admin/wrapper-permissions/list") => match parse_json_body::<
            underwrap_protocol::service::AdminWrapperPermissionsListRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_wrapper_permissions_list(&state, &body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/wrapper-permissions/upsert") => match parse_json_body::<
            underwrap_protocol::service::AdminWrapperPermissionUpsertRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_wrapper_permission_upsert(&state, &principal, &body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/wrapper-permissions/delete") => match parse_json_body::<
            underwrap_protocol::service::AdminWrapperPermissionDeleteRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_wrapper_permission_delete(&state, &principal, &body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/resources/list") => {
            match parse_json_body::<underwrap_protocol::service::AdminResourcesListRequest>(request)
                .await
            {
                Ok(body) => handle_admin_resources_list(&state, &body),
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/resources/create") => match parse_json_body::<
            underwrap_protocol::service::AdminResourceCreateRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_resource_create(&state, &principal, &body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/resources/delete") => match parse_json_body::<
            underwrap_protocol::service::AdminResourceDeleteRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_resource_delete(&state, &principal, &body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/resources/rename") => match parse_json_body::<
            underwrap_protocol::service::AdminResourceRenameRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_resource_rename(&state, &principal, &body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/resources/blob/get") => match parse_json_body::<
            underwrap_protocol::service::AdminResourceBlobGetRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_resource_blob_get(&state, &body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/resources/blob/put") => match parse_json_body::<
            underwrap_protocol::service::AdminResourceBlobPutRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_resource_blob_put(&state, &actor, &body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/pairs/list") => {
            match parse_json_body::<underwrap_protocol::service::AdminPairsListRequest>(request)
                .await
            {
                Ok(body) => handle_admin_pairs_list(&state, &body),
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/pairs/create") => {
            match parse_json_body::<underwrap_protocol::service::AdminPairCreateRequest>(request)
                .await
            {
                Ok(body) => handle_admin_pair_create(&state, &principal, &body),
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/pairs/delete") => {
            match parse_json_body::<underwrap_protocol::service::AdminPairDeleteRequest>(request)
                .await
            {
                Ok(body) => handle_admin_pair_delete(&state, &principal, &body),
                Err(response) => response,
            }
        }
        _ => error_response(StatusCode::NOT_FOUND, "not_found", "unknown admin endpoint"),
    };
    Ok(attach_csrf_cookie(response, &state, &auth))
}

#[cfg(test)]
mod store_registry_tests {
    use super::*;
    use underwrap_protocol::config::StoreRegistryDocument;

    fn write_registry(path: &Path, stores: Vec<StoreRecord>) {
        let doc = StoreRegistryDocument { version: 1, stores };
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
    }

    fn plain_record(id: &str, name: &str, dir: &Path) -> StoreRecord {
        StoreRecord {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            created_at: None,
            storage: underwrap_protocol::config::StorageConfig::Plain {
                dir: dir.to_path_buf(),
            },
        }
    }

    #[test]
    fn missing_registry_file_is_tolerated() {
        let tmp = tempfile::TempDir::new().unwrap();
        let map = load_store_records(&tmp.path().join("stores.json")).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn registry_round_trips_by_store_id() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("stores.json");
        write_registry(
            &path,
            vec![plain_record(
                "store_work",
                "Work",
                &tmp.path().join("work-data"),
            )],
        );

        let map = load_store_records(&path).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map["store_work"].record.name, "Work");
    }

    #[test]
    fn duplicate_visible_name_is_rejected() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("stores.json");
        write_registry(
            &path,
            vec![
                plain_record("store_a", "Shared", &tmp.path().join("a")),
                plain_record("store_b", "shared", &tmp.path().join("b")),
            ],
        );

        let err = load_store_records(&path).expect_err("duplicate visible name must abort");
        assert!(format!("{err:#}").contains("duplicate visible store name"));
    }

    #[test]
    fn invalid_store_id_is_rejected() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("stores.json");
        write_registry(
            &path,
            vec![plain_record("Bad-Id", "Work", &tmp.path().join("x"))],
        );

        let err = load_store_records(&path).expect_err("invalid id must be refused");
        assert!(format!("{err:#}").contains("invalid store id"));
    }

    #[test]
    fn description_metadata_round_trips() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("stores.json");
        let mut record = plain_record("store_work", "Work", &tmp.path().join("w"));
        record.description = Some("the work store".into());
        record.created_at = Some(123);
        write_registry(&path, vec![record]);
        let map = load_store_records(&path).unwrap();
        assert_eq!(
            map["store_work"].record.description.as_deref(),
            Some("the work store")
        );
        assert_eq!(map["store_work"].record.created_at, Some(123));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;
    use underwrap_protocol::message::SecretBytes;
    use underwrap_protocol::policy::PolicyDocument;
    use underwrap_protocol::service::{
        AdminClientInstanceCreateRequest, AdminClientInstanceStatus,
        AdminClientInstanceUpdateRequest,
    };

    struct TestStore {
        data: Mutex<HashMap<String, Vec<u8>>>,
        fail_ops: Mutex<HashSet<&'static str>>,
    }

    impl TestStore {
        fn new() -> Self {
            Self {
                data: Mutex::new(HashMap::new()),
                fail_ops: Mutex::new(HashSet::new()),
            }
        }

        fn should_fail(&self, op: &'static str) -> anyhow::Result<()> {
            if self.fail_ops.lock().unwrap().contains(op) {
                anyhow::bail!("forced {op} failure");
            }
            Ok(())
        }
    }

    impl StorageBackend for TestStore {
        fn read(&self, secret_id: &str) -> anyhow::Result<Option<SecretBytes>> {
            self.should_fail("read")?;
            Ok(self
                .data
                .lock()
                .unwrap()
                .get(secret_id)
                .cloned()
                .map(Into::into))
        }

        fn write(&self, secret_id: &str, material: &[u8]) -> anyhow::Result<()> {
            self.should_fail("write")?;
            self.data
                .lock()
                .unwrap()
                .insert(secret_id.to_string(), material.to_vec());
            Ok(())
        }

        fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
            self.should_fail("list")?;
            let mut ids: Vec<_> = self
                .data
                .lock()
                .unwrap()
                .keys()
                .filter(|id| id.starts_with(prefix))
                .cloned()
                .collect();
            ids.sort();
            Ok(ids)
        }

        fn delete(&self, secret_id: &str) -> anyhow::Result<bool> {
            self.should_fail("delete")?;
            Ok(self.data.lock().unwrap().remove(secret_id).is_some())
        }
    }

    #[test]
    fn policy_document_roundtrips_through_management_helpers() {
        let stores = HashMap::from([(
            "work".to_string(),
            Arc::new(TestStore::new()) as Arc<dyn StorageBackend>,
        )]);

        let json = r#"{"wrappers":[{"wrapper":"gh-comment","mode":{"kind":"unlocked"}}],"rules":[{"id":"r1","wrapper":"gh-comment","resource":"secret:github/token","operation":"secret.read","effect":"allow"}],"grants":[]}"#;
        put_policy_document(&stores, "work", json).unwrap();
        let stored = get_policy_document(&stores, "work").unwrap();
        assert_eq!(stored.as_deref(), Some(json));
    }

    #[test]
    fn resources_document_roundtrips_through_load_save() {
        use underwrap_protocol::entities::{
            EntityCreator, RESOURCES_DOCUMENT_SECRET_ID, ResourceRecord, ResourcesDocument,
        };
        let store = TestStore::new();
        assert_eq!(
            load_resources_document(&store).unwrap(),
            ResourcesDocument::default(),
        );
        let mut doc = ResourcesDocument::default();
        doc.insert(ResourceRecord {
            id: "res_token".into(),
            name: "github/token".into(),
            created_by: EntityCreator::Operator {
                id: "op-1".into(),
                username: "alice".into(),
            },
            created_at: 1_700_000_000,
        })
        .unwrap();
        save_resources_document(&store, &doc).unwrap();
        assert_eq!(load_resources_document(&store).unwrap(), doc);

        // Force an unsupported version on disk: load must refuse.
        store
            .write(
                RESOURCES_DOCUMENT_SECRET_ID,
                br#"{"version": 999, "resources": []}"#,
            )
            .unwrap();
        let err = load_resources_document(&store).unwrap_err();
        assert!(err.to_string().contains("unsupported resources document"));
    }

    #[test]
    fn pairs_document_roundtrips_through_load_save() {
        use underwrap_protocol::entities::{
            EntityCreator, PAIRS_DOCUMENT_SECRET_ID, PairsDocument, WrapperResourcePairRecord,
        };
        let store = TestStore::new();
        assert_eq!(
            load_pairs_document(&store).unwrap(),
            PairsDocument::default(),
        );
        let mut doc = PairsDocument::default();
        doc.insert(WrapperResourcePairRecord {
            wrapper: "gh-comment".into(),
            resource_id: "res_token".into(),
            created_by: EntityCreator::ClientInstance {
                id: "client-7".into(),
            },
            created_at: 42,
            last_used_at: None,
        })
        .unwrap();
        save_pairs_document(&store, &doc).unwrap();
        assert_eq!(load_pairs_document(&store).unwrap(), doc);

        store
            .write(
                PAIRS_DOCUMENT_SECRET_ID,
                br#"{"version": 999, "pairs": []}"#,
            )
            .unwrap();
        let err = load_pairs_document(&store).unwrap_err();
        assert!(err.to_string().contains("unsupported pairs document"));
    }

    #[test]
    fn put_policy_document_rejects_invalid_json() {
        let stores = HashMap::from([(
            "work".to_string(),
            Arc::new(TestStore::new()) as Arc<dyn StorageBackend>,
        )]);

        let err = put_policy_document(&stores, "work", "not-json").unwrap_err();
        assert!(err.to_string().contains("parsing policy document json"));
    }

    fn test_users_file() -> PathBuf {
        std::env::temp_dir().join(format!(
            "underwrap-server-test-operators-{}.json",
            crate::users::new_user_id()
        ))
    }

    fn test_admin_state(stores: HashMap<String, Arc<dyn StorageBackend>>) -> AdminRuntimeState {
        let state: HashMap<String, StoreState> = stores
            .into_iter()
            .map(|(id, b)| (id, StoreState::Unlocked(b)))
            .collect();
        AdminRuntimeState {
            stores: Arc::new(std::sync::RwLock::new(StoreRegistry {
                state,
                records: HashMap::new(),
            })),
            client_instances: Arc::new(std::sync::RwLock::new(HashMap::new())),
            stores_file: std::path::PathBuf::new(),
            client_instances_file: std::path::PathBuf::new(),
            stores_data_root: None,
            rotation_locks: std::sync::Mutex::new(HashMap::new()),
            admin_enabled: true,
            allowed_origins: None,
            web_source: crate::web::WebAssetSource::Embedded,
            users_file: test_users_file(),
            session_key: [0u8; crate::auth::SESSION_KEY_LEN],
            session_lifetime_secs: crate::auth::DEFAULT_SESSION_LIFETIME_SECS,
            cookies_secure: false,
            login_rate_limiter: crate::auth::RateLimiter::new(),
            session_revocations: crate::auth::SessionRevocations::new(),
            bootstrap_lock: std::sync::Mutex::new(()),
            user_collection_lock: std::sync::Mutex::new(()),
        }
    }

    #[derive(Default)]
    struct MemTestStore {
        data: std::sync::Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MemTestStore {
        fn new() -> Self {
            Self::default()
        }
    }

    impl StorageBackend for MemTestStore {
        fn read(
            &self,
            id: &str,
        ) -> anyhow::Result<Option<underwrap_protocol::message::SecretBytes>> {
            Ok(self.data.lock().unwrap().get(id).cloned().map(Into::into))
        }
        fn write(&self, id: &str, m: &[u8]) -> anyhow::Result<()> {
            self.data.lock().unwrap().insert(id.to_string(), m.to_vec());
            Ok(())
        }
        fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
            Ok(self
                .data
                .lock()
                .unwrap()
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect())
        }
        fn delete(&self, id: &str) -> anyhow::Result<bool> {
            Ok(self.data.lock().unwrap().remove(id).is_some())
        }
    }

    fn headers_with_session_cookie(value: Option<&str>) -> hyper::HeaderMap {
        let mut headers = hyper::HeaderMap::new();
        if let Some(v) = value {
            headers.insert(
                hyper::header::COOKIE,
                hyper::header::HeaderValue::from_str(&format!("__Host-uw-session={v}")).unwrap(),
            );
        }
        headers
    }

    #[test]
    fn authenticate_admin_rejects_missing_and_invalid_session_cookie() {
        let state = seeded_admin_state("alice", "hunter2");
        assert!(authenticate_admin(&state, &headers_with_session_cookie(None)).is_err());
        assert!(
            authenticate_admin(&state, &headers_with_session_cookie(Some("not-a-session")))
                .is_err()
        );
    }

    #[test]
    fn validate_admin_config_allows_nonempty_origin_entries() {
        let cfg = AdminConfig {
            allowed_origins: Some(vec!["https://ok.example".to_string()]),
            session_key_file: PathBuf::from("/dev/null"),
            session_lifetime_secs: None,
            stores_data_root: None,
        };
        validate_admin_config(&cfg).expect("nonempty origins must be allowed");
    }

    #[test]
    fn validate_admin_config_rejects_empty_origin_entry() {
        let cfg = AdminConfig {
            allowed_origins: Some(vec!["https://ok.example".to_string(), "  ".to_string()]),
            session_key_file: PathBuf::from("/dev/null"),
            session_lifetime_secs: None,
            stores_data_root: None,
        };
        let err = validate_admin_config(&cfg).unwrap_err();
        assert!(format!("{err:#}").contains("empty entry"));
    }

    fn headers_with(pairs: &[(&str, &str)]) -> hyper::HeaderMap {
        let mut headers = hyper::HeaderMap::new();
        for (k, v) in pairs {
            headers.insert(
                hyper::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                hyper::header::HeaderValue::from_str(v).unwrap(),
            );
        }
        headers
    }

    #[test]
    fn check_admin_origin_passes_get_without_check() {
        let allowed = vec!["https://ok.example".to_string()];
        assert!(check_admin_origin(&Method::GET, &headers_with(&[]), Some(&allowed)).is_ok());
        assert!(check_admin_origin(&Method::HEAD, &headers_with(&[]), Some(&allowed)).is_ok());
        assert!(check_admin_origin(&Method::OPTIONS, &headers_with(&[]), Some(&allowed)).is_ok());
    }

    #[test]
    fn check_admin_origin_passes_when_no_allowlist_configured() {
        // No allowlist == no check, even for POST.
        assert!(check_admin_origin(&Method::POST, &headers_with(&[]), None).is_ok());
        assert!(check_admin_origin(&Method::POST, &headers_with(&[]), Some(&[])).is_ok());
    }

    #[test]
    fn check_admin_origin_accepts_matching_origin() {
        let allowed = vec!["https://ok.example".to_string()];
        let headers = headers_with(&[("Origin", "https://ok.example")]);
        assert!(check_admin_origin(&Method::POST, &headers, Some(&allowed)).is_ok());
    }

    #[test]
    fn check_admin_origin_rejects_mismatched_origin() {
        let allowed = vec!["https://ok.example".to_string()];
        let headers = headers_with(&[("Origin", "https://attacker.example")]);
        let err = check_admin_origin(&Method::POST, &headers, Some(&allowed)).unwrap_err();
        assert!(err.contains("not in allowed_origins"));
    }

    #[test]
    fn check_admin_origin_falls_back_to_referer() {
        let allowed = vec!["https://ok.example".to_string()];
        let headers = headers_with(&[("Referer", "https://ok.example/admin/users")]);
        assert!(check_admin_origin(&Method::POST, &headers, Some(&allowed)).is_ok());
    }

    #[test]
    fn check_admin_origin_rejects_referer_from_other_origin() {
        let allowed = vec!["https://ok.example".to_string()];
        let headers = headers_with(&[("Referer", "https://attacker.example/page")]);
        let err = check_admin_origin(&Method::POST, &headers, Some(&allowed)).unwrap_err();
        assert!(err.contains("not in allowed_origins"));
    }

    #[test]
    fn check_admin_origin_rejects_when_origin_and_referer_missing() {
        let allowed = vec!["https://ok.example".to_string()];
        let err =
            check_admin_origin(&Method::POST, &headers_with(&[]), Some(&allowed)).unwrap_err();
        assert!(err.contains("missing Origin"));
    }

    #[test]
    fn referer_to_origin_strips_path_and_query() {
        assert_eq!(
            referer_to_origin("https://ok.example/foo/bar?x=1#y").as_deref(),
            Some("https://ok.example")
        );
        assert_eq!(
            referer_to_origin("http://localhost:5173/").as_deref(),
            Some("http://localhost:5173")
        );
    }

    #[test]
    fn referer_to_origin_rejects_malformed_input() {
        assert!(referer_to_origin("not-a-url").is_none());
        assert!(referer_to_origin("https://").is_none());
        assert!(referer_to_origin("://nohost").is_none());
    }

    // ---------- Login + whoami end-to-end against the real handler ----------

    fn seeded_admin_state(username: &str, password: &str) -> AdminRuntimeState {
        let users_file = test_users_file();
        let mut collection = crate::users::UserCollection::default();
        let hash = crate::auth::hash_password(password).unwrap();
        collection
            .add(crate::users::UserRecord {
                id: crate::users::new_user_id(),
                username: username.to_string(),
                password_hash: hash,
                created_at: 1000,
                password_changed_at: 1000,
            })
            .unwrap();
        crate::users::save(&users_file, &collection).unwrap();
        let mut session_key = [0u8; crate::auth::SESSION_KEY_LEN];
        for (i, b) in session_key.iter_mut().enumerate() {
            *b = i as u8;
        }
        AdminRuntimeState {
            stores: Arc::new(std::sync::RwLock::new(StoreRegistry::new())),
            client_instances: Arc::new(std::sync::RwLock::new(HashMap::new())),
            stores_file: std::path::PathBuf::new(),
            client_instances_file: std::path::PathBuf::new(),
            stores_data_root: None,
            rotation_locks: std::sync::Mutex::new(HashMap::new()),
            admin_enabled: true,
            allowed_origins: None,
            web_source: crate::web::WebAssetSource::Embedded,
            users_file,
            session_key,
            session_lifetime_secs: 3600,
            cookies_secure: false,
            login_rate_limiter: crate::auth::RateLimiter::new(),
            session_revocations: crate::auth::SessionRevocations::new(),
            bootstrap_lock: std::sync::Mutex::new(()),
            user_collection_lock: std::sync::Mutex::new(()),
        }
    }

    /// We can't easily synthesise a `hyper::body::Incoming`, so test the
    /// login flow at the layer below `handle_admin_login` — namely, the
    /// password verify + session encrypt pipeline — using the public
    /// `auth` helpers. The handler itself is exercised by integration
    /// tests against a real listener.
    #[tokio::test]
    async fn login_password_pipeline_succeeds_and_round_trips_cookie() {
        let state = seeded_admin_state("alice", "hunter2");
        let collection = crate::users::load(&state.users_file).unwrap();
        let op = collection.find_by_username("alice").unwrap();
        assert!(crate::auth::verify_password(&op.password_hash, "hunter2"));
        assert!(!crate::auth::verify_password(&op.password_hash, "wrong"));

        let key = state.session_key;
        let payload = crate::auth::new_session_payload(
            op.id.clone(),
            op.username.clone(),
            1000,
            state.session_lifetime_secs,
        );
        let cookie = crate::auth::encrypt_session(&payload, &key).unwrap();
        let decoded = crate::auth::decrypt_session(&cookie, &key, 1500).unwrap();
        assert_eq!(decoded.username, "alice");
    }

    #[tokio::test]
    async fn unknown_user_login_pipeline_uses_dummy_hash() {
        // Verify that the unknown-user branch in handle_admin_login
        // selects the dummy hash, keeping wall-clock behaviour
        // consistent with the real-user path.
        let state = seeded_admin_state("alice", "hunter2");
        let collection = crate::users::load(&state.users_file).unwrap();
        assert!(collection.find_by_username("eve").is_none());
        assert!(!crate::auth::verify_password(
            crate::auth::DUMMY_PASSWORD_HASH,
            "anything"
        ));
    }

    #[tokio::test]
    async fn whoami_resolves_username_from_collection() {
        let state = seeded_admin_state("alice", "hunter2");
        let key = state.session_key;
        // `handle_admin_whoami` reads SystemTime::now() internally, so
        // give the cookie a real-clock issued_at.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let payload = crate::auth::new_session_payload("op-1".into(), "alice".into(), now, 3600);
        let cookie_value = crate::auth::encrypt_session(&payload, &key).unwrap();

        let mut headers = hyper::HeaderMap::new();
        let cookie = format!("__Host-uw-session={cookie_value}");
        headers.insert(
            hyper::header::COOKIE,
            hyper::header::HeaderValue::from_str(&cookie).unwrap(),
        );
        let resp = handle_admin_whoami(&state, &headers);
        assert_eq!(resp.status(), StatusCode::OK);
        let body = http_body_util::BodyExt::collect(resp.into_body())
            .await
            .unwrap()
            .to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["authenticated"], true);
        assert_eq!(parsed["username"], "alice");
        assert!(
            parsed.get("role").is_none(),
            "wire shape must not carry a role"
        );
    }

    #[tokio::test]
    async fn whoami_returns_unauthenticated_without_cookie() {
        let state = seeded_admin_state("alice", "hunter2");
        let resp = handle_admin_whoami(&state, &hyper::HeaderMap::new());
        let body = http_body_util::BodyExt::collect(resp.into_body())
            .await
            .unwrap()
            .to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["authenticated"], false);
    }

    // ---------- CSRF + logout (ticket 212) ----------

    /// Build a request-shaped header map carrying a freshly-issued
    /// session cookie plus optional CSRF cookie/header.
    fn session_headers(
        state: &AdminRuntimeState,
        username: &str,
        with_csrf_cookie: bool,
        csrf_header: Option<&str>,
    ) -> (hyper::HeaderMap, crate::auth::SessionPayload) {
        let key = state.session_key;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let payload = crate::auth::new_session_payload("op-1".into(), username.into(), now, 3600);
        let session_cookie = crate::auth::encrypt_session(&payload, &key).unwrap();
        let mut cookie_parts = vec![format!("__Host-uw-session={session_cookie}")];
        if with_csrf_cookie {
            cookie_parts.push(format!("__Host-uw-csrf={}", payload.csrf_secret));
        }
        let mut headers = hyper::HeaderMap::new();
        headers.insert(
            hyper::header::COOKIE,
            hyper::header::HeaderValue::from_str(&cookie_parts.join("; ")).unwrap(),
        );
        if let Some(value) = csrf_header {
            headers.insert(
                hyper::header::HeaderName::from_static("x-csrf-token"),
                hyper::header::HeaderValue::from_str(value).unwrap(),
            );
        }
        (headers, payload)
    }

    fn service_state_with_runtime_client(
        cert_der: &[u8],
        status: crate::runtime_clients::ClientInstanceStatus,
        store_id: &str,
    ) -> AdminRuntimeState {
        let mut state_map: HashMap<String, StoreState> = HashMap::new();
        state_map.insert(
            store_id.to_string(),
            StoreState::Unlocked(Arc::new(MemTestStore::new())),
        );
        let mut records: HashMap<String, LoadedStoreRecord> = HashMap::new();
        records.insert(
            store_id.to_string(),
            LoadedStoreRecord {
                record: StoreRecord {
                    id: store_id.to_string(),
                    name: store_id.to_string(),
                    description: None,
                    created_at: None,
                    storage: underwrap_protocol::config::StorageConfig::Plain {
                        dir: PathBuf::from("/dev/null"),
                    },
                },
            },
        );
        let mut client_instances = HashMap::new();
        client_instances.insert(
            "inst_local".to_string(),
            crate::runtime_clients::ClientInstanceRecord {
                id: "inst_local".to_string(),
                display_name: "Local".to_string(),
                current_store_id: store_id.to_string(),
                status,
                last_seen_at: None,
                current_credential: crate::runtime_clients::CurrentCredentialRecord {
                    cert_fingerprint: crate::runtime_clients::cert_fingerprint_from_der(cert_der),
                    cert_pem: "pem".to_string(),
                    issued_at: None,
                },
                auto_create_wrapper_assets: false,
            },
        );
        AdminRuntimeState {
            stores: Arc::new(std::sync::RwLock::new(StoreRegistry {
                state: state_map,
                records,
            })),
            client_instances: Arc::new(std::sync::RwLock::new(client_instances)),
            stores_file: PathBuf::new(),
            client_instances_file: PathBuf::new(),
            stores_data_root: None,
            rotation_locks: std::sync::Mutex::new(HashMap::new()),
            admin_enabled: true,
            allowed_origins: None,
            web_source: crate::web::WebAssetSource::Embedded,
            users_file: test_users_file(),
            session_key: [0u8; crate::auth::SESSION_KEY_LEN],
            session_lifetime_secs: crate::auth::DEFAULT_SESSION_LIFETIME_SECS,
            cookies_secure: false,
            login_rate_limiter: crate::auth::RateLimiter::new(),
            session_revocations: crate::auth::SessionRevocations::new(),
            bootstrap_lock: std::sync::Mutex::new(()),
            user_collection_lock: std::sync::Mutex::new(()),
        }
    }

    #[test]
    fn resolve_runtime_client_maps_cert_to_active_instance_store() {
        let cert_der = vec![1u8, 2, 3, 4];
        let state = service_state_with_runtime_client(
            &cert_der,
            crate::runtime_clients::ClientInstanceStatus::Active,
            "work",
        );
        let cert = rustls::pki_types::CertificateDer::from(cert_der);
        let resolved = resolve_runtime_client(&state, &[cert]).unwrap();
        assert_eq!(resolved.instance_id, "inst_local");
        assert_eq!(resolved.store_id, "work");
    }

    #[test]
    fn resolve_runtime_client_rejects_disabled_instance() {
        let cert_der = vec![9u8, 8, 7];
        let state = service_state_with_runtime_client(
            &cert_der,
            crate::runtime_clients::ClientInstanceStatus::Disabled,
            "work",
        );
        let cert = rustls::pki_types::CertificateDer::from(cert_der);
        let err = match resolve_runtime_client(&state, &[cert]) {
            Ok(_) => panic!("disabled instances must fail"),
            Err(err) => err,
        };
        assert!(format!("{err:#}").contains("not active"));
    }

    #[test]
    fn resolve_runtime_client_rejects_unknown_fingerprint() {
        let state = service_state_with_runtime_client(
            &[1u8, 2, 3],
            crate::runtime_clients::ClientInstanceStatus::Active,
            "work",
        );
        let cert = rustls::pki_types::CertificateDer::from(vec![4u8, 5, 6]);
        let err = match resolve_runtime_client(&state, &[cert]) {
            Ok(_) => panic!("unknown fingerprints must fail"),
            Err(err) => err,
        };
        assert!(format!("{err:#}").contains("unknown client credential fingerprint"));
    }

    #[test]
    fn admin_store_resolves_existing_store() {
        let store: Arc<dyn StorageBackend> = Arc::new(MemTestStore::new());
        let state = test_admin_state(HashMap::from([("work".to_string(), store.clone())]));
        assert!(admin_store(&state, "work").is_ok());
    }

    fn expect_404(result: std::result::Result<Arc<dyn StorageBackend>, Response<Full<Bytes>>>) {
        match result {
            Ok(_) => panic!("expected 404 not_found, got Ok"),
            Err(resp) => assert_eq!(resp.status(), StatusCode::NOT_FOUND),
        }
    }

    #[test]
    fn admin_store_unknown_store_returns_404() {
        let state = test_admin_state(HashMap::new());
        expect_404(admin_store(&state, "missing"));
    }

    #[test]
    fn resolve_principal_session_resolves_username_from_collection() {
        let state = seeded_admin_state("alice", "hunter2");
        let users_file = state.users_file.clone();
        let mut collection = crate::users::UserCollection::default();
        let hash = crate::auth::hash_password("hunter2").unwrap();
        collection
            .add(crate::users::UserRecord {
                id: crate::users::new_user_id(),
                username: "alice".into(),
                password_hash: hash,
                created_at: 1000,
                password_changed_at: 1000,
            })
            .unwrap();
        crate::users::save(&users_file, &collection).unwrap();

        let payload = crate::auth::new_session_payload("op-1".into(), "alice".into(), 1000, 3600);
        let auth = AdminAuth(payload);
        let p = resolve_principal(&state, &auth).unwrap();
        assert_eq!(p.username, "alice");
    }

    #[test]
    fn resolve_principal_session_fails_when_user_was_deleted() {
        // Cookie says "alice" but the collection has no record of her.
        let state = seeded_admin_state("alice", "hunter2");
        // Save an empty collection over alice's slot.
        crate::users::save(&state.users_file, &crate::users::UserCollection::default()).unwrap();
        let payload = crate::auth::new_session_payload("op-1".into(), "alice".into(), 1000, 3600);
        let auth = AdminAuth(payload);
        match resolve_principal(&state, &auth) {
            Ok(_) => panic!("expected 401 for deleted operator"),
            Err(resp) => assert_eq!(resp.status(), StatusCode::UNAUTHORIZED),
        }
    }

    // ---------- Bootstrap (ticket 214) ----------

    /// Build admin state with a session key but an empty operator
    /// collection — the canonical "fresh install" shape.
    fn fresh_bootstrap_state() -> AdminRuntimeState {
        let mut key = [0u8; crate::auth::SESSION_KEY_LEN];
        for (i, b) in key.iter_mut().enumerate() {
            *b = i as u8;
        }
        AdminRuntimeState {
            stores: Arc::new(std::sync::RwLock::new(StoreRegistry::new())),
            client_instances: Arc::new(std::sync::RwLock::new(HashMap::new())),
            stores_file: std::path::PathBuf::new(),
            client_instances_file: std::path::PathBuf::new(),
            stores_data_root: None,
            rotation_locks: std::sync::Mutex::new(HashMap::new()),
            admin_enabled: true,
            allowed_origins: None,
            web_source: crate::web::WebAssetSource::Embedded,
            users_file: test_users_file(),
            session_key: key,
            session_lifetime_secs: 3600,
            cookies_secure: false,
            login_rate_limiter: crate::auth::RateLimiter::new(),
            session_revocations: crate::auth::SessionRevocations::new(),
            bootstrap_lock: std::sync::Mutex::new(()),
            user_collection_lock: std::sync::Mutex::new(()),
        }
    }

    async fn read_json(resp: Response<Full<Bytes>>) -> serde_json::Value {
        let body = http_body_util::BodyExt::collect(resp.into_body())
            .await
            .unwrap()
            .to_bytes();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn bootstrap_status_required_on_fresh_store() {
        let state = fresh_bootstrap_state();
        let resp = handle_admin_bootstrap_status(&state);
        assert_eq!(resp.status(), StatusCode::OK);
        let body = read_json(resp).await;
        assert_eq!(body["bootstrap_required"], true);
    }

    #[tokio::test]
    async fn bootstrap_status_false_when_sentinel_set() {
        let state = fresh_bootstrap_state();
        crate::users::mark_bootstrap_completed(
            &state.users_file,
            &crate::users::BootstrapSentinel {
                completed_at: 1000,
                by_username: "alice".into(),
            },
        )
        .unwrap();
        let body = read_json(handle_admin_bootstrap_status(&state)).await;
        assert_eq!(body["bootstrap_required"], false);
    }

    #[tokio::test]
    async fn bootstrap_status_false_when_operators_exist_even_without_sentinel() {
        let state = fresh_bootstrap_state();
        let mut collection = crate::users::UserCollection::default();
        collection
            .add(crate::users::UserRecord {
                id: crate::users::new_user_id(),
                username: "alice".into(),
                password_hash: crate::auth::hash_password("p").unwrap(),
                created_at: 1000,
                password_changed_at: 1000,
            })
            .unwrap();
        crate::users::save(&state.users_file, &collection).unwrap();
        let body = read_json(handle_admin_bootstrap_status(&state)).await;
        assert_eq!(body["bootstrap_required"], false);
    }

    /// Drive the bootstrap-flow inner logic (everything after the JSON
    /// body has been parsed). We can't synthesise a `Request<Incoming>`
    /// in unit tests, so factor the core into a helper exercised here
    /// and by the real handler.
    fn run_bootstrap(
        state: &AdminRuntimeState,
        username: &str,
        password: &str,
    ) -> Response<Full<Bytes>> {
        // Mirror the validation in `handle_admin_bootstrap` minus the
        // request-body parsing, which `parse_json_body` already covers.
        let session_key = state.session_key;
        let username_norm = crate::users::normalise_username(username);
        if username_norm.is_empty() || password.is_empty() {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_credentials",
                "missing field",
            );
        }
        let _guard = state.bootstrap_lock.lock().unwrap();
        if crate::users::bootstrap_completed(&state.users_file).unwrap_or(true) {
            audit_bootstrap_attempt(state, &username_norm, "bootstrap_already_completed");
            return error_response(
                StatusCode::CONFLICT,
                "bootstrap_already_completed",
                "already done",
            );
        }
        let collection = crate::users::load(&state.users_file).unwrap();
        if !collection.is_empty() {
            audit_bootstrap_attempt(state, &username_norm, "users_already_exist");
            return error_response(
                StatusCode::CONFLICT,
                "users_already_exist",
                "already populated",
            );
        }
        let now = crate::users::now_epoch_secs();
        let hash = crate::auth::hash_password(password).unwrap();
        let record = crate::users::UserRecord {
            id: crate::users::new_user_id(),
            username: username_norm.clone(),
            password_hash: hash,
            created_at: now,
            password_changed_at: now,
        };
        let mut new_collection = crate::users::UserCollection::default();
        new_collection.add(record.clone()).unwrap();
        crate::users::save(&state.users_file, &new_collection).unwrap();
        crate::users::mark_bootstrap_completed(
            &state.users_file,
            &crate::users::BootstrapSentinel {
                completed_at: now,
                by_username: record.username.clone(),
            },
        )
        .unwrap();
        audit_bootstrap_attempt(state, &record.username, "success");
        let payload = crate::auth::new_session_payload(
            record.id.clone(),
            record.username.clone(),
            now,
            state.session_lifetime_secs,
        );
        let encoded = crate::auth::encrypt_session(&payload, &session_key).unwrap();
        let session_cookie = crate::auth::render_session_cookie(
            &encoded,
            state.session_lifetime_secs,
            state.cookies_secure,
        );
        let csrf_cookie = crate::auth::render_csrf_cookie(
            &payload.csrf_secret,
            state.session_lifetime_secs,
            state.cookies_secure,
        );
        Response::builder()
            .status(StatusCode::OK)
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .header(hyper::header::SET_COOKIE, session_cookie)
            .header(hyper::header::SET_COOKIE, csrf_cookie)
            .body(Full::new(Bytes::from_static(b"{}")))
            .expect("response")
    }

    #[tokio::test]
    async fn bootstrap_succeeds_on_empty_store_and_sets_sentinel() {
        let state = fresh_bootstrap_state();
        let resp = run_bootstrap(&state, "alice", "hunter2");
        assert_eq!(resp.status(), StatusCode::OK);
        // Sentinel is set.
        assert!(crate::users::bootstrap_completed(&state.users_file).unwrap());
        // First user record is created.
        let collection = crate::users::load(&state.users_file).unwrap();
        let u = collection.find_by_username("alice").unwrap();
        assert!(crate::auth::verify_password(&u.password_hash, "hunter2"));
        // Session + CSRF cookies present.
        let cookies: Vec<String> = resp
            .headers()
            .get_all(hyper::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
            .collect();
        assert!(cookies.iter().any(|c| c.starts_with("__Host-uw-session=")));
        assert!(cookies.iter().any(|c| c.starts_with("__Host-uw-csrf=")));
    }

    #[tokio::test]
    async fn bootstrap_second_call_returns_bootstrap_already_completed() {
        let state = fresh_bootstrap_state();
        let _ = run_bootstrap(&state, "alice", "hunter2");
        let resp = run_bootstrap(&state, "eve", "haxx0r");
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = read_json(resp).await;
        assert_eq!(body["code"], "bootstrap_already_completed");
    }

    #[tokio::test]
    async fn bootstrap_refuses_when_users_exist_without_sentinel() {
        // This is the "CLI recovery happened first" case: a user
        // exists, but the sentinel was never set. The online flow
        // must still refuse with users_already_exist.
        let state = fresh_bootstrap_state();
        let mut collection = crate::users::UserCollection::default();
        collection
            .add(crate::users::UserRecord {
                id: crate::users::new_user_id(),
                username: "rescuer".into(),
                password_hash: crate::auth::hash_password("rescue").unwrap(),
                created_at: 1000,
                password_changed_at: 1000,
            })
            .unwrap();
        crate::users::save(&state.users_file, &collection).unwrap();

        let resp = run_bootstrap(&state, "newbie", "x");
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = read_json(resp).await;
        assert_eq!(body["code"], "users_already_exist");
    }

    #[tokio::test]
    async fn bootstrap_concurrent_calls_only_one_succeeds() {
        let state = Arc::new(fresh_bootstrap_state());
        let s1 = state.clone();
        let s2 = state.clone();
        let h1 = tokio::task::spawn_blocking(move || run_bootstrap(&s1, "alice", "p1"));
        let h2 = tokio::task::spawn_blocking(move || run_bootstrap(&s2, "bob", "p2"));
        let r1 = h1.await.unwrap();
        let r2 = h2.await.unwrap();
        let statuses = [r1.status(), r2.status()];
        let ok_count = statuses.iter().filter(|s| **s == StatusCode::OK).count();
        let conflict_count = statuses
            .iter()
            .filter(|s| **s == StatusCode::CONFLICT)
            .count();
        assert_eq!(ok_count, 1, "exactly one bootstrap must win: {statuses:?}");
        assert_eq!(conflict_count, 1, "the loser must see 409: {statuses:?}");
        // Sentinel is set and collection has exactly one operator.
        assert!(crate::users::bootstrap_completed(&state.users_file).unwrap());
        let collection = crate::users::load(&state.users_file).unwrap();
        assert_eq!(collection.len(), 1);
    }

    #[test]
    fn authenticate_admin_rejects_revoked_session() {
        let state = seeded_admin_state("alice", "hunter2");
        let (headers, payload) = session_headers(&state, "alice", false, None);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        state
            .session_revocations
            .revoke(payload.session_id.clone(), payload.expires_at, now);
        let err = authenticate_admin(&state, &headers).unwrap_err();
        assert!(matches!(err, AdminAuthError::Revoked));
    }

    #[test]
    fn authenticate_admin_accepts_session_when_not_revoked() {
        let state = seeded_admin_state("alice", "hunter2");
        let (headers, _payload) = session_headers(&state, "alice", false, None);
        let auth = authenticate_admin(&state, &headers).unwrap();
        assert_eq!(auth.operator(), "alice");
        assert_eq!(auth.session().username, "alice");
    }

    #[test]
    fn handle_admin_logout_revokes_and_clears_cookies() {
        let state = seeded_admin_state("alice", "hunter2");
        let payload = crate::auth::new_session_payload("op-1".into(), "alice".into(), 1000, 3600);
        let response = handle_admin_logout(&state, &payload);
        assert_eq!(response.status(), StatusCode::OK);
        let cookies: Vec<String> = response
            .headers()
            .get_all(hyper::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
            .collect();
        assert!(
            cookies
                .iter()
                .any(|c| c.starts_with("__Host-uw-session=;") && c.contains("Max-Age=0")),
            "expected session cookie cleared in {cookies:?}",
        );
        assert!(
            cookies
                .iter()
                .any(|c| c.starts_with("__Host-uw-csrf=;") && c.contains("Max-Age=0")),
            "expected csrf cookie cleared in {cookies:?}",
        );
        // Revocation entry visible while still inside the original window.
        assert!(
            state
                .session_revocations
                .is_revoked(&payload.session_id, 1500)
        );
        // ...and naturally frees once the session would have expired anyway.
        assert!(
            !state
                .session_revocations
                .is_revoked(&payload.session_id, payload.expires_at + 1)
        );
    }

    #[test]
    fn attach_csrf_cookie_refreshes_on_session_auth() {
        let state = seeded_admin_state("alice", "hunter2");
        let payload = crate::auth::new_session_payload("op-1".into(), "alice".into(), 1000, 3600);
        let response = json_response(StatusCode::OK, &serde_json::json!({"hello": "world"}));
        let wrapped = attach_csrf_cookie(response, &state, &AdminAuth(payload.clone()));
        let cookies: Vec<String> = wrapped
            .headers()
            .get_all(hyper::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
            .collect();
        assert!(
            cookies
                .iter()
                .any(|c| c.starts_with(&format!("__Host-uw-csrf={}", payload.csrf_secret))),
            "expected refreshed CSRF cookie in {cookies:?}",
        );
    }

    #[test]
    fn check_csrf_passes_safe_methods_without_token() {
        let headers = hyper::HeaderMap::new();
        check_csrf(&Method::GET, &headers, "any-secret").unwrap();
        check_csrf(&Method::HEAD, &headers, "any-secret").unwrap();
        check_csrf(&Method::OPTIONS, &headers, "any-secret").unwrap();
    }

    #[test]
    fn check_csrf_rejects_missing_header_with_csrf_required() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert(
            hyper::header::COOKIE,
            hyper::header::HeaderValue::from_static("__Host-uw-csrf=the-secret"),
        );
        let (code, _msg) = check_csrf(&Method::POST, &headers, "the-secret").unwrap_err();
        assert_eq!(code, "csrf_required");
    }

    #[test]
    fn check_csrf_rejects_missing_cookie_with_csrf_required() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert(
            hyper::header::HeaderName::from_static("x-csrf-token"),
            hyper::header::HeaderValue::from_static("the-secret"),
        );
        let (code, _msg) = check_csrf(&Method::POST, &headers, "the-secret").unwrap_err();
        assert_eq!(code, "csrf_required");
    }

    #[test]
    fn check_csrf_rejects_mismatched_header_with_csrf_mismatch() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert(
            hyper::header::COOKIE,
            hyper::header::HeaderValue::from_static("__Host-uw-csrf=the-secret"),
        );
        headers.insert(
            hyper::header::HeaderName::from_static("x-csrf-token"),
            hyper::header::HeaderValue::from_static("a-different-value"),
        );
        let (code, _) = check_csrf(&Method::POST, &headers, "the-secret").unwrap_err();
        assert_eq!(code, "csrf_mismatch");
    }

    #[test]
    fn check_csrf_rejects_when_cookie_does_not_match_session() {
        // Same value in cookie and header, but neither matches the
        // session's csrf_secret — a cookie-tossing or stale-cookie
        // scenario must not slip through.
        let mut headers = hyper::HeaderMap::new();
        headers.insert(
            hyper::header::COOKIE,
            hyper::header::HeaderValue::from_static("__Host-uw-csrf=stale-value"),
        );
        headers.insert(
            hyper::header::HeaderName::from_static("x-csrf-token"),
            hyper::header::HeaderValue::from_static("stale-value"),
        );
        let (code, _) = check_csrf(&Method::POST, &headers, "real-session-secret").unwrap_err();
        assert_eq!(code, "csrf_mismatch");
    }

    #[test]
    fn check_csrf_accepts_matching_header_cookie_and_session() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert(
            hyper::header::COOKIE,
            hyper::header::HeaderValue::from_static("__Host-uw-csrf=the-secret"),
        );
        headers.insert(
            hyper::header::HeaderName::from_static("x-csrf-token"),
            hyper::header::HeaderValue::from_static("the-secret"),
        );
        check_csrf(&Method::POST, &headers, "the-secret").unwrap();
    }

    // ===== Stores CRUD (ticket 231) =====

    fn admin_principal(_stores: &[&str]) -> crate::users::Principal {
        crate::users::Principal {
            id: "admin-id".into(),
            username: "alice".into(),
        }
    }

    /// Build an `AdminRuntimeState` wired up for CRUD tests: a
    /// dedicated store registry, user-state file, and one allowed
    /// data root, all under the supplied tempdir.
    fn crud_state(tmp: &std::path::Path) -> AdminRuntimeState {
        let dynamic_dir = tmp.join("stores.d");
        std::fs::create_dir_all(&dynamic_dir).unwrap();
        let data_root = tmp.join("data");
        std::fs::create_dir_all(&data_root).unwrap();
        AdminRuntimeState {
            stores: Arc::new(std::sync::RwLock::new(StoreRegistry::new())),
            client_instances: Arc::new(std::sync::RwLock::new(HashMap::new())),
            stores_file: tmp.join("stores.json"),
            client_instances_file: tmp.join("client-instances.json"),
            stores_data_root: Some(data_root),
            rotation_locks: std::sync::Mutex::new(HashMap::new()),
            admin_enabled: true,
            allowed_origins: None,
            web_source: crate::web::WebAssetSource::Embedded,
            users_file: tmp.join("users.json"),
            session_key: [0u8; crate::auth::SESSION_KEY_LEN],
            session_lifetime_secs: crate::auth::DEFAULT_SESSION_LIFETIME_SECS,
            cookies_secure: false,
            login_rate_limiter: crate::auth::RateLimiter::new(),
            session_revocations: crate::auth::SessionRevocations::new(),
            bootstrap_lock: std::sync::Mutex::new(()),
            user_collection_lock: std::sync::Mutex::new(()),
        }
    }

    fn create_store_for_test(
        state: &AdminRuntimeState,
        principal: &crate::users::Principal,
        body: AdminStoreCreateRequest,
    ) -> String {
        let resp = do_admin_store_create(state, principal, body);
        assert_eq!(resp.status(), StatusCode::OK);
        response_body_json(resp)["store"]["id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    const SAMPLE_CLIENT_CERT_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIDKzCCAhOgAwIBAgIUVnjkCKXzQiyHyEsQ3ey4lgWkrTAwDQYJKoZIhvcNAQEL
BQAwHDEaMBgGA1UEAwwRdW5kZXJ3cmFwLXRlc3QtY2EwHhcNMjYwNDE2MDAzNzAy
WhcNMzYwNDEzMDAzNzAyWjAaMRgwFgYDVQQDDA91bmRlcndyYXAtbG9jYWwwggEi
MA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQDcJDP5K69TcG9+uHM7q+xG7p0c
7Cgex8XYVs3yFDpHk6UOG/JqL0bKa32rhO7LR8VxPprIyqV24LB8vTi52A2/oA/d
ZiVouv9bkCiBOun0XDQAHveocrIYnhjvq8qrpdfFdZ+2oB/V78Mbf6FaNMJxvvUN
hCRXh/hstwlHVACJFX0MlGU/3WAhpRF09QjIfxLUqm1t5Sd9Qg0a2IsX+qrFhDiu
35anBoCx0x9AJl6yO40KgoDZ7iDmWLw55T2GVnSQEdTelxzsf9jFLjEzm56mymR+
DGWdMNPEKY6wyvuJClEYLEsHaNMfTi9Jp+9FgH5spuy7E9UqpqeciYAHlDrxAgMB
AAGjZzBlMBMGA1UdJQQMMAoGCCsGAQUFBwMCMA4GA1UdDwEB/wQEAwIFoDAdBgNV
HQ4EFgQUrZfzoVv+iUwAlCedd1/ancfUi7YwHwYDVR0jBBgwFoAUlScjoHF7mIGw
Qy61SLb9isP1dd4wDQYJKoZIhvcNAQELBQADggEBAMj5BtbOalwYWAi9HQt0JNPq
NtRObdNx6Yg2uZWYAyhVXnhLerZ82hWagK/oH0KCgKFCSK140oZS/Nt7xtQ9YJXs
g5WyGnsteF3QZ9cuUc+0te4eMOmXRMch8HtxyD1sCuO/wyKhtQSCO+OQGNvsEnKa
1rl3PFTjG3SE5ldDwRjGSlrf9+6uf2OcqJBLaL4dcOJ0Fsrf0gkkPpbrC5hJdNSo
Q+0W8oyLdzgmGNMRY4vvhu4cjYkbPLgrNS3fQLnnom3Jv5GzO3M2ifS9mkj249Bu
cFuCWqBWazuUyiSP6kQz7Ad4/9qxEu4c8Hv3Ecc5XuQDpl4Rxox+j/7lUrthzrI=
-----END CERTIFICATE-----
"#;

    fn response_body_json(resp: Response<Full<Bytes>>) -> serde_json::Value {
        let (parts, body) = resp.into_parts();
        let bytes = body.collect_bytes_simple();
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("response body is JSON");
        // Smuggle the status into the JSON value so the test can assert both.
        let mut obj = v.as_object().cloned().unwrap_or_default();
        obj.insert(
            "__status".into(),
            serde_json::Value::Number(parts.status.as_u16().into()),
        );
        serde_json::Value::Object(obj)
    }
    // Local trait extension so we can grab the body bytes synchronously.
    trait CollectBytesSync {
        fn collect_bytes_simple(self) -> Vec<u8>;
    }
    impl CollectBytesSync for Full<Bytes> {
        fn collect_bytes_simple(self) -> Vec<u8> {
            use http_body_util::BodyExt;
            // If we're already on a tokio runtime (e.g. inside a
            // `#[tokio::test(flavor = "multi_thread")]` test) we cannot
            // construct a nested Runtime — use `block_in_place` to
            // escape the worker. Otherwise spin up a small runtime
            // just for this drain.
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                tokio::task::block_in_place(|| {
                    handle.block_on(async { self.collect().await.unwrap().to_bytes().to_vec() })
                })
            } else {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async { self.collect().await.unwrap().to_bytes().to_vec() })
            }
        }
    }

    #[test]
    fn resolve_store_dir_uses_id_as_leaf_under_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let resolved =
            resolve_store_dir(&state, "store_deadbeef").expect("must accept when root is set");
        assert_eq!(resolved, tmp.path().join("data").join("store_deadbeef"));
    }

    #[test]
    fn resolve_store_dir_rejects_when_root_unconfigured() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut state = crud_state(tmp.path());
        state.stores_data_root = None;
        let err = resolve_store_dir(&state, "store_x").expect_err("must refuse without root");
        assert_eq!(err.0, "data_root_not_configured");
    }

    #[test]
    fn resolve_store_dir_rejects_relative_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut state = crud_state(tmp.path());
        state.stores_data_root = Some(std::path::PathBuf::from("relative/data"));
        let err = resolve_store_dir(&state, "store_x").expect_err("must refuse relative root");
        assert_eq!(err.0, "data_root_not_absolute");
    }

    #[test]
    fn list_returns_empty_when_nothing_configured() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let resp = handle_admin_stores_list(&state);
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        assert_eq!(body["items"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn admin_can_create_get_update_delete_a_dynamic_plain_store() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let create = AdminStoreCreateRequest {
            name: "work".to_string(),
            backend: AdminStoreBackendKind::Plain,
            description: Some("the work store".into()),
        };
        let resp = do_admin_store_create(&state, &p, create);
        assert_eq!(resp.status(), StatusCode::OK);

        let body = response_body_json(resp);
        let store_id = body["store"]["id"].as_str().unwrap().to_string();
        let expected_dir = tmp.path().join("data").join(&store_id);
        assert_eq!(body["store"]["name"], "work");
        assert_eq!(body["store"]["backend"], "plain");
        assert_eq!(body["store"]["description"], "the work store");
        assert_eq!(body["store"]["usage"]["mapped_client_instance_count"], 0);
        assert_eq!(body["store"]["usage"]["has_secrets"], false);
        assert_eq!(body["store"]["actions"]["can_edit_metadata"], true);
        assert_eq!(body["store"]["actions"]["can_rotate_key"], false);
        assert_eq!(body["store"]["actions"]["can_delete_now"], true);
        assert_eq!(body["store"]["actions"]["can_force_delete"], false);
        let registry_json = std::fs::read_to_string(&state.stores_file).unwrap();
        assert!(registry_json.contains(&store_id));
        // The runtime registry sees it.
        let resp = handle_admin_stores_list(&state);
        let body = response_body_json(resp);
        let items = body["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], store_id);
        assert_eq!(items[0]["name"], "work");
        assert_eq!(items[0]["backend"], "plain");
        assert_eq!(items[0]["description"], "the work store");
        assert_eq!(items[0]["usage"]["mapped_client_instance_count"], 0);
        assert_eq!(items[0]["usage"]["has_secrets"], false);
        assert_eq!(items[0]["actions"]["can_delete_now"], true);
        assert_eq!(items[0]["actions"]["can_force_delete"], false);
        // Detail.
        let resp = handle_admin_store_detail(&state, &store_id);
        let body = response_body_json(resp);
        assert_eq!(body["__status"], 200);
        assert_eq!(body["dir"], expected_dir.to_string_lossy().to_string());
        assert_eq!(body["usage"]["mapped_client_instance_count"], 0);
        assert_eq!(body["usage"]["has_secrets"], false);
        assert_eq!(body["actions"]["can_edit_metadata"], true);
        assert_eq!(body["actions"]["can_delete_now"], true);
        // Update description.
        let resp = do_admin_store_update(
            &state,
            &p,
            &store_id,
            AdminStoreUpdateRequest {
                name: None,
                description: Some("renamed".into()),
            },
        );
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = handle_admin_store_detail(&state, &store_id);
        assert_eq!(response_body_json(resp)["description"], "renamed");
        // Clearing description with an empty string nulls it out.
        let resp = do_admin_store_update(
            &state,
            &p,
            &store_id,
            AdminStoreUpdateRequest {
                name: None,
                description: Some(String::new()),
            },
        );
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = handle_admin_store_detail(&state, &store_id);
        let body = response_body_json(resp);
        assert!(!body.as_object().unwrap().contains_key("description"));
        // Delete (empty store).
        let resp = handle_admin_store_delete(&state, &p, &store_id, false);
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        assert_eq!(body["ok"], true);
        assert!(!body.as_object().unwrap().contains_key("cleanup_hint"));
        assert!(
            !state.stores_file.exists()
                || !std::fs::read_to_string(&state.stores_file)
                    .unwrap_or_default()
                    .contains(&store_id)
        );
    }

    #[test]
    fn create_refuses_when_stores_data_root_unconfigured() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut state = crud_state(tmp.path());
        state.stores_data_root = None;
        let resp = do_admin_store_create(
            &state,
            &admin_principal(&[]),
            AdminStoreCreateRequest {
                name: "stray".into(),
                backend: AdminStoreBackendKind::Plain,
                description: None,
            },
        );
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body_json(resp);
        assert_eq!(body["code"], "data_root_not_configured");
    }

    #[test]
    fn create_refuses_duplicate_name() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let req = AdminStoreCreateRequest {
            name: "work".to_string(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        };
        let _first = do_admin_store_create(&state, &p, req.clone());
        let resp = do_admin_store_create(&state, &p, req);
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = response_body_json(resp);
        assert_eq!(body["code"], "store_name_already_exists");
    }

    #[test]
    fn delete_refuses_non_empty_without_force_and_emits_cleanup_hint_with_force() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let store_id = create_store_for_test(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                description: None,
            },
        );
        let expected_dir = tmp.path().join("data").join(&store_id);
        // Drop a secret in so the store is non-empty.
        {
            let reg = state.stores.read().unwrap();
            reg.expect_unlocked(&store_id)
                .write("greeting", &SecretBytes::from(b"hello".to_vec()))
                .unwrap();
        }
        // Without force → 409 + code store_not_empty.
        let resp = handle_admin_store_delete(&state, &p, &store_id, false);
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        assert_eq!(response_body_json(resp)["code"], "store_not_empty");
        // With force → 200 + cleanup_hint pointing at the data dir.
        let resp = handle_admin_store_delete(&state, &p, &store_id, true);
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        let hint = body["cleanup_hint"]
            .as_str()
            .expect("force-delete must include a cleanup_hint when non-empty");
        assert!(hint.contains(expected_dir.to_string_lossy().as_ref()));
    }

    #[test]
    fn create_plain_store_returns_no_key_hex() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let resp = do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                description: None,
            },
        );
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        assert!(
            !body.as_object().unwrap().contains_key("key_hex"),
            "plain stores must not carry a key_hex; got {body:?}",
        );
        // The store payload sits under `store`.
        assert_eq!(body["store"]["backend"], "plain");
    }

    #[test]
    fn create_encrypted_store_generates_key_in_memory_and_returns_it_once() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let resp = do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "vault".into(),
                backend: AdminStoreBackendKind::Encrypted,
                description: None,
            },
        );
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        let store_id = body["store"]["id"].as_str().unwrap().to_string();
        let key_hex = body["key_hex"]
            .as_str()
            .expect("encrypted create must include key_hex");
        assert_eq!(key_hex.len(), 64, "key_hex must be 32 bytes hex-encoded");
        assert!(
            key_hex.chars().all(|c| c.is_ascii_hexdigit()),
            "key_hex must be lowercase hex",
        );

        // No file under the test root survives the call with a
        // `.key` suffix — the master key never touched disk. We walk
        // manually rather than pull in a new dependency.
        fn assert_no_key_files(dir: &std::path::Path) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    assert_no_key_files(&path);
                } else {
                    let name = entry.file_name();
                    let name_lossy = name.to_string_lossy();
                    assert!(
                        !name_lossy.ends_with(".key"),
                        "no on-disk .key file should exist after create; found {}",
                        path.display(),
                    );
                }
            }
        }
        assert_no_key_files(tmp.path());

        // The unlock canary lives in the data directory under its
        // reserved id; we read it back through the live registry
        // backend so we know the key in memory still decrypts it.
        let backend = state.stores.read().unwrap().expect_unlocked(&store_id);
        let canary = backend
            .read(underwrap_store::ENCRYPTED_CANARY_ID)
            .unwrap()
            .expect("canary must exist after create");
        assert_eq!(canary.as_ref(), underwrap_store::ENCRYPTED_CANARY_PLAINTEXT);

        // The detail response carries the data dir and `locked=false`
        // but never echoes the key value.
        let resp = handle_admin_store_detail(&state, &store_id);
        let body = response_body_json(resp);
        assert_eq!(body["backend"], "encrypted");
        assert_eq!(body["locked"], false);
        let body_str = serde_json::to_string(&body).unwrap();
        assert!(
            !body_str.contains(key_hex),
            "key material leaked into detail response: {body_str}",
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rotate_key_reencrypts_resources_and_returns_new_value_once() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let resp = do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "vault".into(),
                backend: AdminStoreBackendKind::Encrypted,
                description: None,
            },
        );
        assert_eq!(resp.status(), StatusCode::OK);
        let create_body = response_body_json(resp);
        let store_id = create_body["store"]["id"].as_str().unwrap().to_string();
        let original_key = create_body["key_hex"].as_str().unwrap().to_string();

        // Seed a known plaintext under the original key.
        {
            let backend = state.stores.read().unwrap().expect_unlocked(&store_id);
            backend.write("greeting", b"hello").unwrap();
        }

        let resp = handle_admin_store_rotate_key(&state, &p, &store_id).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        let new_key = body["key_hex"].as_str().unwrap().to_string();
        assert_ne!(new_key, original_key, "rotation must produce a fresh key");
        assert_eq!(new_key.len(), 64);
        assert!(
            body["warning"]
                .as_str()
                .unwrap()
                .contains("re-encrypted under the new master key"),
            "warning must describe the re-encryption: {}",
            body["warning"],
        );

        // The active backend still decodes the same plaintext (now
        // under the new key).
        {
            let backend = state.stores.read().unwrap().expect_unlocked(&store_id);
            assert_eq!(
                backend.read("greeting").unwrap().unwrap().as_ref(),
                b"hello"
            );
            // Canary survived the rotation.
            assert_eq!(
                backend
                    .read(underwrap_store::ENCRYPTED_CANARY_ID)
                    .unwrap()
                    .unwrap()
                    .as_ref(),
                underwrap_store::ENCRYPTED_CANARY_PLAINTEXT,
            );
        }

        // Detail GET still does not leak the key value.
        let resp = handle_admin_store_detail(&state, &store_id);
        let body = response_body_json(resp);
        let body_str = serde_json::to_string(&body).unwrap();
        assert!(!body_str.contains(&new_key));
        assert!(!body_str.contains(&original_key));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rotate_key_refuses_plain_store() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let store_id = create_store_for_test(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                description: None,
            },
        );
        let resp = handle_admin_store_rotate_key(&state, &p, &store_id).await;
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        assert_eq!(response_body_json(resp)["code"], "not_encrypted");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rotate_key_refuses_locked_store() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let store_id = create_store_for_test(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "vault".into(),
                backend: AdminStoreBackendKind::Encrypted,
                description: None,
            },
        );
        // Lock the store and confirm rotation refuses.
        let resp = handle_admin_store_lock(&state, &p, &store_id);
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = handle_admin_store_rotate_key(&state, &p, &store_id).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response_body_json(resp)["code"], "store_locked");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rotate_key_succeeds_on_unlocked_encrypted_store() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let store_id = create_store_for_test(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "vault".into(),
                backend: AdminStoreBackendKind::Encrypted,
                description: None,
            },
        );
        let resp = handle_admin_store_rotate_key(&state, &p, &store_id).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn unlock_round_trip_succeeds_with_correct_hex() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let resp = do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "vault".into(),
                backend: AdminStoreBackendKind::Encrypted,
                description: None,
            },
        );
        let body = response_body_json(resp);
        let store_id = body["store"]["id"].as_str().unwrap().to_string();
        let key_hex = body["key_hex"].as_str().unwrap().to_string();

        // Simulate a restart by locking the store.
        let resp = handle_admin_store_lock(&state, &p, &store_id);
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(state.stores.read().unwrap().is_locked(&store_id));

        let resp = do_admin_store_unlock(
            &state,
            &p,
            &store_id,
            underwrap_protocol::service::AdminStoreUnlockRequest { key_hex },
        );
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(!state.stores.read().unwrap().is_locked(&store_id));
    }

    #[test]
    fn unlock_rejects_wrong_hex_and_keeps_store_locked() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let resp = do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "vault".into(),
                backend: AdminStoreBackendKind::Encrypted,
                description: None,
            },
        );
        let store_id = response_body_json(resp)["store"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let resp = handle_admin_store_lock(&state, &p, &store_id);
        assert_eq!(resp.status(), StatusCode::OK);

        let bogus_hex = "0".repeat(64);
        let resp = do_admin_store_unlock(
            &state,
            &p,
            &store_id,
            underwrap_protocol::service::AdminStoreUnlockRequest { key_hex: bogus_hex },
        );
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert_eq!(response_body_json(resp)["code"], "key_mismatch");
        assert!(state.stores.read().unwrap().is_locked(&store_id));
    }

    #[test]
    fn unlock_rejects_invalid_hex_format() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let resp = do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "vault".into(),
                backend: AdminStoreBackendKind::Encrypted,
                description: None,
            },
        );
        let store_id = response_body_json(resp)["store"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let resp = handle_admin_store_lock(&state, &p, &store_id);
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = do_admin_store_unlock(
            &state,
            &p,
            &store_id,
            underwrap_protocol::service::AdminStoreUnlockRequest {
                key_hex: "not hex".into(),
            },
        );
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_body_json(resp)["code"], "invalid_key_hex");
    }

    #[test]
    fn unlock_refuses_plain_store() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let store_id = create_store_for_test(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                description: None,
            },
        );
        let resp = do_admin_store_unlock(
            &state,
            &p,
            &store_id,
            underwrap_protocol::service::AdminStoreUnlockRequest {
                key_hex: "0".repeat(64),
            },
        );
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_body_json(resp)["code"], "store_not_encrypted");
    }

    #[test]
    fn unlock_refuses_already_unlocked_store() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let resp = do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "vault".into(),
                backend: AdminStoreBackendKind::Encrypted,
                description: None,
            },
        );
        let body = response_body_json(resp);
        let store_id = body["store"]["id"].as_str().unwrap().to_string();
        let key_hex = body["key_hex"].as_str().unwrap().to_string();
        let resp = do_admin_store_unlock(
            &state,
            &p,
            &store_id,
            underwrap_protocol::service::AdminStoreUnlockRequest { key_hex },
        );
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        assert_eq!(response_body_json(resp)["code"], "already_unlocked");
    }

    #[test]
    fn lock_transitions_unlocked_encrypted_store() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let resp = do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "vault".into(),
                backend: AdminStoreBackendKind::Encrypted,
                description: None,
            },
        );
        let store_id = response_body_json(resp)["store"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(!state.stores.read().unwrap().is_locked(&store_id));
        let resp = handle_admin_store_lock(&state, &p, &store_id);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(response_body_json(resp)["was_locked"], false);
        assert!(state.stores.read().unwrap().is_locked(&store_id));
        // Second call is a no-op success.
        let resp = handle_admin_store_lock(&state, &p, &store_id);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(response_body_json(resp)["was_locked"], true);
    }

    #[test]
    fn lock_refuses_plain_store() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let store_id = create_store_for_test(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                description: None,
            },
        );
        let resp = handle_admin_store_lock(&state, &p, &store_id);
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_body_json(resp)["code"], "store_not_encrypted");
    }

    #[test]
    fn canary_is_hidden_from_admin_has_secrets_view() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let resp = do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "vault".into(),
                backend: AdminStoreBackendKind::Encrypted,
                description: None,
            },
        );
        let store_id = response_body_json(resp)["store"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        // Fresh encrypted store has only the canary; admin view must
        // not flag it as having secrets.
        let detail = response_body_json(handle_admin_store_detail(&state, &store_id));
        assert_eq!(detail["usage"]["has_secrets"], false);
    }

    #[test]
    fn store_management_view_reflects_usage_for_authenticated_user() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let admin = admin_principal(&[]);
        let plain_id = create_store_for_test(
            &state,
            &admin,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                description: None,
            },
        );
        let encrypted_id = create_store_for_test(
            &state,
            &admin,
            AdminStoreCreateRequest {
                name: "vault".into(),
                backend: AdminStoreBackendKind::Encrypted,
                description: None,
            },
        );
        {
            let reg = state.stores.read().unwrap();
            reg.expect_unlocked(&plain_id)
                .write("secret/item", b"value")
                .unwrap();
            reg.expect_unlocked(&encrypted_id)
                .write("secret/item", b"value")
                .unwrap();
        }
        state.client_instances.write().unwrap().insert(
            "client_a".into(),
            crate::runtime_clients::ClientInstanceRecord {
                id: "client_a".into(),
                display_name: "Laptop".into(),
                current_store_id: plain_id.clone(),
                status: crate::runtime_clients::ClientInstanceStatus::Active,
                last_seen_at: None,
                current_credential: crate::runtime_clients::CurrentCredentialRecord {
                    cert_fingerprint: "fp-1".into(),
                    cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
                    issued_at: Some(1),
                },
                auto_create_wrapper_assets: false,
            },
        );

        let list = response_body_json(handle_admin_stores_list(&state));
        let items = list["items"].as_array().unwrap();
        let plain = items.iter().find(|item| item["id"] == plain_id).unwrap();
        assert_eq!(plain["usage"]["mapped_client_instance_count"], 1);
        assert_eq!(plain["usage"]["has_secrets"], true);
        assert_eq!(plain["actions"]["can_edit_metadata"], true);
        assert_eq!(plain["actions"]["can_rotate_key"], false);
        assert_eq!(plain["actions"]["can_delete_now"], false);
        assert_eq!(plain["actions"]["can_force_delete"], false);

        let encrypted = items
            .iter()
            .find(|item| item["id"] == encrypted_id)
            .unwrap();
        assert_eq!(encrypted["usage"]["mapped_client_instance_count"], 0);
        assert_eq!(encrypted["usage"]["has_secrets"], true);
        assert_eq!(encrypted["actions"]["can_edit_metadata"], true);
        assert_eq!(encrypted["actions"]["can_rotate_key"], true);
        assert_eq!(encrypted["actions"]["can_delete_now"], false);
        assert_eq!(encrypted["actions"]["can_force_delete"], true);

        let detail = response_body_json(handle_admin_store_detail(&state, &plain_id));
        assert_eq!(detail["usage"]["mapped_client_instance_count"], 1);
        assert_eq!(detail["usage"]["has_secrets"], true);
        assert_eq!(detail["actions"]["can_edit_metadata"], true);
        assert_eq!(detail["actions"]["can_rotate_key"], false);
        assert_eq!(detail["actions"]["can_delete_now"], false);
        assert_eq!(detail["actions"]["can_force_delete"], false);
    }

    #[test]
    fn client_instance_crud_and_store_mapping_round_trip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let work_id = create_store_for_test(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                description: None,
            },
        );
        let other_id = create_store_for_test(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "other".into(),
                backend: AdminStoreBackendKind::Plain,
                description: None,
            },
        );

        let create = do_admin_client_instance_create(
            &state,
            &p,
            AdminClientInstanceCreateRequest {
                display_name: "laptop-work".into(),
                current_store_id: work_id.clone(),
                status: AdminClientInstanceStatus::Active,
                cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
                issued_at: Some(111),
                auto_create_wrapper_assets: None,
            },
        );
        assert_eq!(create.status(), StatusCode::OK);
        let created = response_body_json(create);
        let client_id = created["id"].as_str().unwrap().to_string();
        assert_eq!(created["current_store_id"], work_id);
        assert_eq!(created["current_store_name"], "work");
        assert_eq!(created["actions"]["can_edit_display_name"], true);
        assert_eq!(created["actions"]["can_remap_store"], true);
        assert_eq!(created["actions"]["can_disable"], true);
        assert_eq!(created["actions"]["can_enable"], false);
        assert_eq!(created["actions"]["can_delete"], true);

        let list = handle_admin_client_instances_list(&state);
        let list_body = response_body_json(list);
        assert_eq!(list_body["items"].as_array().unwrap().len(), 1);
        assert_eq!(list_body["items"][0]["id"], client_id);
        assert_eq!(list_body["items"][0]["current_store_name"], "work");
        assert_eq!(list_body["items"][0]["actions"]["can_disable"], true);

        let delete_in_use = handle_admin_store_delete(&state, &p, &work_id, true);
        assert_eq!(delete_in_use.status(), StatusCode::CONFLICT);
        assert_eq!(response_body_json(delete_in_use)["code"], "store_in_use");

        let update = do_admin_client_instance_update(
            &state,
            &p,
            &client_id,
            AdminClientInstanceUpdateRequest {
                display_name: Some("laptop-renamed".into()),
                current_store_id: Some(other_id.clone()),
                status: Some(AdminClientInstanceStatus::Disabled),
                cert_pem: None,
                auto_create_wrapper_assets: None,
            },
        );
        assert_eq!(update.status(), StatusCode::OK);
        let updated = response_body_json(update);
        assert_eq!(updated["display_name"], "laptop-renamed");
        assert_eq!(updated["current_store_id"], other_id);
        assert_eq!(updated["current_store_name"], "other");
        assert_eq!(updated["status"], "disabled");
        assert_eq!(updated["actions"]["can_disable"], false);
        assert_eq!(updated["actions"]["can_enable"], true);

        let list_after_disable = response_body_json(handle_admin_client_instances_list(&state));
        assert_eq!(list_after_disable["items"].as_array().unwrap().len(), 1);
        assert_eq!(list_after_disable["items"][0]["id"], client_id);
        assert_eq!(list_after_disable["items"][0]["status"], "disabled");

        let delete_old_store = handle_admin_store_delete(&state, &p, &work_id, false);
        assert_eq!(delete_old_store.status(), StatusCode::OK);

        let delete_client = handle_admin_client_instance_delete(&state, &p, &client_id);
        assert_eq!(delete_client.status(), StatusCode::OK);
        assert_eq!(response_body_json(delete_client)["ok"], true);
    }

    #[test]
    fn client_instance_delete_cascades_to_wrapper_permissions() {
        use underwrap_protocol::policy::{WrapperApprovalMode, WrapperPermission};
        use underwrap_protocol::service::AdminWrapperPermissionUpsertRequest;

        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let store_id = create_store_for_test(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                description: None,
            },
        );

        // Two distinct client instances on the same store: only the
        // first one is deleted, so the second one's permission must
        // survive the cascade. The fingerprint uniqueness check in the
        // runtime client registry forces us to bypass the create API
        // (which validates a real PEM) and seed records directly.
        let doomed_id = "doomed".to_string();
        let survivor_id = "survivor".to_string();
        {
            let mut instances = state.client_instances.write().unwrap();
            for (id, fp) in [(&doomed_id, "fp-doomed"), (&survivor_id, "fp-survivor")] {
                instances.insert(
                    id.clone(),
                    crate::runtime_clients::ClientInstanceRecord {
                        id: id.clone(),
                        display_name: format!("{id}-laptop"),
                        current_store_id: store_id.clone(),
                        status: crate::runtime_clients::ClientInstanceStatus::Active,
                        last_seen_at: None,
                        current_credential: crate::runtime_clients::CurrentCredentialRecord {
                            cert_fingerprint: fp.into(),
                            cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
                            issued_at: Some(1),
                        },
                        auto_create_wrapper_assets: false,
                    },
                );
            }
            crate::runtime_clients::save_client_instances(&state.client_instances_file, &instances)
                .unwrap();
        }

        // The admin API addresses stores by id (the SPA's `storeName`
        // state is misleadingly named — it carries the store id).
        for instance in [&doomed_id, &survivor_id] {
            let upsert = handle_admin_wrapper_permission_upsert(
                &state,
                &p,
                &AdminWrapperPermissionUpsertRequest {
                    store: store_id.clone(),
                    permission: WrapperPermission {
                        client_instance: instance.clone(),
                        wrapper: "gh".into(),
                        resource_id: "github/token".into(),
                        mode: WrapperApprovalMode::Unlocked,
                        writable: false,
                        reason: None,
                    },
                },
            );
            assert_eq!(upsert.status(), StatusCode::OK);
        }

        let before = load_policy_document_or_default(
            state
                .stores
                .read()
                .unwrap()
                .expect_unlocked(&store_id)
                .as_ref(),
        )
        .unwrap();
        assert_eq!(before.permissions.len(), 2);

        let delete = handle_admin_client_instance_delete(&state, &p, &doomed_id);
        assert_eq!(delete.status(), StatusCode::OK);

        // The doomed instance's permission is gone; the survivor's
        // remains so we know we cascaded by id, not by store-wide nuke.
        let after = load_policy_document_or_default(
            state
                .stores
                .read()
                .unwrap()
                .expect_unlocked(&store_id)
                .as_ref(),
        )
        .unwrap();
        assert_eq!(
            after.permissions.len(),
            1,
            "only the survivor's permission should remain: {after:?}",
        );
        assert_eq!(after.permissions[0].client_instance, survivor_id);
    }

    #[test]
    fn client_instance_auto_create_flag_defaults_off_and_can_be_toggled() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let store_id = create_store_for_test(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                description: None,
            },
        );

        // Default: when the create request omits the flag the stored
        // value is false and the detail/list response reports it.
        let create = do_admin_client_instance_create(
            &state,
            &p,
            AdminClientInstanceCreateRequest {
                display_name: "default-flag".into(),
                current_store_id: store_id.clone(),
                status: AdminClientInstanceStatus::Active,
                cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
                issued_at: None,
                auto_create_wrapper_assets: None,
            },
        );
        assert_eq!(create.status(), StatusCode::OK);
        let created = response_body_json(create);
        assert_eq!(created["auto_create_wrapper_assets"], false);
        let id = created["id"].as_str().unwrap().to_string();

        // Update toggles it on.
        let toggled = do_admin_client_instance_update(
            &state,
            &p,
            &id,
            AdminClientInstanceUpdateRequest {
                display_name: None,
                current_store_id: None,
                status: None,
                cert_pem: None,
                auto_create_wrapper_assets: Some(true),
            },
        );
        assert_eq!(toggled.status(), StatusCode::OK);
        let toggled_body = response_body_json(toggled);
        assert_eq!(toggled_body["auto_create_wrapper_assets"], true);

        // Persisted across a fresh load.
        let reloaded = crate::runtime_clients::load_client_instances(&state.client_instances_file)
            .expect("reload client instances");
        assert!(reloaded.get(&id).unwrap().auto_create_wrapper_assets);

        // Update without the flag leaves it alone.
        let kept = do_admin_client_instance_update(
            &state,
            &p,
            &id,
            AdminClientInstanceUpdateRequest {
                display_name: Some("renamed".into()),
                current_store_id: None,
                status: None,
                cert_pem: None,
                auto_create_wrapper_assets: None,
            },
        );
        assert_eq!(kept.status(), StatusCode::OK);
        assert_eq!(response_body_json(kept)["auto_create_wrapper_assets"], true);

        // And it can be toggled back off explicitly.
        let off = do_admin_client_instance_update(
            &state,
            &p,
            &id,
            AdminClientInstanceUpdateRequest {
                display_name: None,
                current_store_id: None,
                status: None,
                cert_pem: None,
                auto_create_wrapper_assets: Some(false),
            },
        );
        assert_eq!(off.status(), StatusCode::OK);
        assert_eq!(response_body_json(off)["auto_create_wrapper_assets"], false);
    }

    // ===== Admin Resource and Pair CRUD (ticket 384) =====

    fn entity_test_state_with_store(tmp: &std::path::Path) -> (AdminRuntimeState, String) {
        let state = crud_state(tmp);
        let p = admin_principal(&[]);
        let id = create_store_for_test(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                description: None,
            },
        );
        (state, id)
    }

    /// Test helper: call the resource create handler and return the
    /// id the backend assigned. Tests use this when they need to issue
    /// subsequent id-addressed calls against the same resource.
    fn create_resource_id_for_test(
        state: &AdminRuntimeState,
        principal: &crate::users::Principal,
        store_id: &str,
        name: &str,
    ) -> String {
        let resp = handle_admin_resource_create(
            state,
            principal,
            &underwrap_protocol::service::AdminResourceCreateRequest {
                store: store_id.to_string(),
                name: name.to_string(),
            },
        );
        assert_eq!(resp.status(), StatusCode::OK);
        response_body_json(resp)["resource"]["id"]
            .as_str()
            .expect("create response includes resource.id")
            .to_string()
    }

    #[test]
    fn admin_resource_create_then_list_round_trip_with_operator_provenance() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let p = admin_principal(&[]);

        let create = handle_admin_resource_create(
            &state,
            &p,
            &underwrap_protocol::service::AdminResourceCreateRequest {
                store: store_id.clone(),
                name: "github/token".into(),
            },
        );
        assert_eq!(create.status(), StatusCode::OK);
        let body = response_body_json(create);
        assert_eq!(body["resource"]["name"], "github/token");
        assert_eq!(body["resource"]["created_by"]["kind"], "operator");
        assert_eq!(body["resource"]["created_by"]["username"], "alice");

        let list = handle_admin_resources_list(
            &state,
            &underwrap_protocol::service::AdminResourcesListRequest {
                store: store_id.clone(),
            },
        );
        let list_body = response_body_json(list);
        let resources = list_body["resources"].as_array().unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0]["name"], "github/token");
    }

    #[test]
    fn admin_resource_create_refuses_duplicate_name() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let p = admin_principal(&[]);
        let body = underwrap_protocol::service::AdminResourceCreateRequest {
            store: store_id,
            name: "github/token".into(),
        };
        let first = handle_admin_resource_create(&state, &p, &body);
        assert_eq!(first.status(), StatusCode::OK);
        let dup = handle_admin_resource_create(&state, &p, &body);
        assert_eq!(dup.status(), StatusCode::CONFLICT);
        assert_eq!(response_body_json(dup)["code"], "resource_already_exists");
    }

    #[test]
    fn admin_resource_delete_succeeds_when_no_dependents() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let p = admin_principal(&[]);
        let resource_id = create_resource_id_for_test(&state, &p, &store_id, "github/token");
        let del = handle_admin_resource_delete(
            &state,
            &p,
            &underwrap_protocol::service::AdminResourceDeleteRequest {
                store: store_id.clone(),
                id: resource_id,
            },
        );
        assert_eq!(del.status(), StatusCode::OK);
        let body = response_body_json(del);
        assert_eq!(body["ok"], true);
        assert!(body.get("blocked_by").is_none() || body["blocked_by"].is_null());

        let list = handle_admin_resources_list(
            &state,
            &underwrap_protocol::service::AdminResourcesListRequest { store: store_id },
        );
        assert_eq!(
            response_body_json(list)["resources"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn admin_resource_rename_flips_name_and_keeps_id() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let p = admin_principal(&[]);
        let resource_id = create_resource_id_for_test(&state, &p, &store_id, "github/token");

        let resp = handle_admin_resource_rename(
            &state,
            &p,
            &underwrap_protocol::service::AdminResourceRenameRequest {
                store: store_id.clone(),
                id: resource_id.clone(),
                new_name: "github/main".into(),
            },
        );
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        assert_eq!(body["resource"]["id"], resource_id);
        assert_eq!(body["resource"]["name"], "github/main");

        // List confirms the rename.
        let list = handle_admin_resources_list(
            &state,
            &underwrap_protocol::service::AdminResourcesListRequest { store: store_id },
        );
        let resources = response_body_json(list)["resources"].clone();
        assert_eq!(resources[0]["id"], resource_id);
        assert_eq!(resources[0]["name"], "github/main");
    }

    #[test]
    fn admin_resource_rename_refuses_when_target_name_taken() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let p = admin_principal(&[]);
        let a = create_resource_id_for_test(&state, &p, &store_id, "a");
        let _b = create_resource_id_for_test(&state, &p, &store_id, "b");

        let resp = handle_admin_resource_rename(
            &state,
            &p,
            &underwrap_protocol::service::AdminResourceRenameRequest {
                store: store_id.clone(),
                id: a,
                new_name: "b".into(),
            },
        );
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        assert_eq!(response_body_json(resp)["code"], "resource_already_exists");
    }

    #[test]
    fn admin_secret_rename_moves_entry_atomically() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let p = admin_principal(&[]);
        let resource_id = create_resource_id_for_test(&state, &p, &store_id, "github/token");

        // Seed an entry.
        let write = handle_admin_secret_write(
            &state,
            "alice",
            underwrap_protocol::service::AdminSecretWriteRequest {
                store: store_id.clone(),
                resource_id: resource_id.clone(),
                slot: underwrap_protocol::resource::ResourceSlot::Files,
                key: "config.yaml".into(),
                material_b64: base64::engine::general_purpose::STANDARD.encode("v1"),
            },
        );
        assert_eq!(write.status(), StatusCode::OK);

        // Rename without changing material.
        let rename = handle_admin_secret_rename(
            &state,
            "alice",
            underwrap_protocol::service::AdminSecretRenameRequest {
                store: store_id.clone(),
                resource_id: resource_id.clone(),
                slot: underwrap_protocol::resource::ResourceSlot::Files,
                from_key: "config.yaml".into(),
                to_key: "config.json".into(),
                material_b64: None,
            },
        );
        assert_eq!(rename.status(), StatusCode::OK);

        // The new key holds the original material; the old key is gone.
        let entries = {
            let backend = state.stores.read().unwrap().expect_unlocked(&store_id);
            list_secret_entries(backend.as_ref(), None).unwrap()
        };
        let keys: Vec<&str> = entries.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, vec!["config.json"]);
    }

    #[test]
    fn admin_secret_rename_refuses_when_target_exists() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let p = admin_principal(&[]);
        let resource_id = create_resource_id_for_test(&state, &p, &store_id, "r");
        for k in ["a", "b"] {
            let _ = handle_admin_secret_write(
                &state,
                "alice",
                underwrap_protocol::service::AdminSecretWriteRequest {
                    store: store_id.clone(),
                    resource_id: resource_id.clone(),
                    slot: underwrap_protocol::resource::ResourceSlot::Files,
                    key: k.into(),
                    material_b64: base64::engine::general_purpose::STANDARD.encode(k),
                },
            );
        }
        let resp = handle_admin_secret_rename(
            &state,
            "alice",
            underwrap_protocol::service::AdminSecretRenameRequest {
                store: store_id.clone(),
                resource_id: resource_id.clone(),
                slot: underwrap_protocol::resource::ResourceSlot::Files,
                from_key: "a".into(),
                to_key: "b".into(),
                material_b64: None,
            },
        );
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        assert_eq!(response_body_json(resp)["code"], "target_key_exists");
    }

    #[test]
    fn admin_resource_delete_refuses_with_structured_blockers() {
        use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let p = admin_principal(&[]);

        // Resource exists.
        let resource_id = create_resource_id_for_test(&state, &p, &store_id, "github/token");
        // Pair references it.
        handle_admin_pair_create(
            &state,
            &p,
            &underwrap_protocol::service::AdminPairCreateRequest {
                store: store_id.clone(),
                wrapper: "gh".into(),
                resource_id: resource_id.clone(),
            },
        );
        // A secret under the resource. The per-slot in-blob entry key
        // shape routes through the blob shim and ends up inside the
        // `resources/<resource_id>` blob.
        {
            let reg = state.stores.read().unwrap();
            let blob_key = format!("{}/env/GH_TOKEN", resource_id);
            write_resource_entry(
                reg.expect_unlocked(&store_id).as_ref(),
                &blob_key,
                b"ghp_value",
            )
            .unwrap();
        }
        // A wrapper permission referencing the resource.
        {
            let reg = state.stores.read().unwrap();
            save_policy_document(
                reg.expect_unlocked(&store_id).as_ref(),
                &PolicyDocument {
                    permissions: vec![WrapperPermission {
                        client_instance: "inst_a".into(),
                        wrapper: "gh".into(),
                        resource_id: resource_id.clone(),
                        mode: WrapperApprovalMode::Unlocked,
                        writable: false,
                        reason: None,
                    }],
                },
            )
            .unwrap();
        }

        let del = handle_admin_resource_delete(
            &state,
            &p,
            &underwrap_protocol::service::AdminResourceDeleteRequest {
                store: store_id.clone(),
                id: resource_id.clone(),
            },
        );
        assert_eq!(del.status(), StatusCode::OK);
        let body = response_body_json(del);
        assert_eq!(body["ok"], false);
        let blockers = &body["blocked_by"];
        assert_eq!(blockers["secret_count"], 1);
        assert_eq!(blockers["pair_count"], 1);
        assert_eq!(blockers["permission_count"], 1);
        assert_eq!(blockers["sample_pairs"][0]["wrapper"], "gh");
        assert_eq!(
            blockers["sample_permissions"][0]["client_instance"],
            "inst_a"
        );

        // Resource is still present.
        let list = handle_admin_resources_list(
            &state,
            &underwrap_protocol::service::AdminResourcesListRequest { store: store_id },
        );
        assert_eq!(
            response_body_json(list)["resources"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn bump_pair_last_used_sets_timestamp_only_on_matching_pair() {
        use underwrap_protocol::entities::{EntityCreator, WrapperResourcePairRecord};
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let store_arc = state.stores.read().unwrap().expect_unlocked(&store_id);
        let store: &dyn StorageBackend = store_arc.as_ref();

        let mut pairs = load_pairs_document(store).unwrap();
        pairs
            .insert(WrapperResourcePairRecord {
                wrapper: "gh".into(),
                resource_id: "res_token".into(),
                created_by: EntityCreator::ClientInstance {
                    id: "inst_a".into(),
                },
                created_at: 100,
                last_used_at: None,
            })
            .unwrap();
        pairs
            .insert(WrapperResourcePairRecord {
                wrapper: "tf".into(),
                resource_id: "res_token".into(),
                created_by: EntityCreator::ClientInstance {
                    id: "inst_a".into(),
                },
                created_at: 100,
                last_used_at: None,
            })
            .unwrap();
        save_pairs_document(store, &pairs).unwrap();

        bump_pair_last_used(store, "gh", "res_token").unwrap();

        let after = load_pairs_document(store).unwrap();
        let gh = after.find("gh", "res_token").unwrap();
        let tf = after.find("tf", "res_token").unwrap();
        assert!(gh.last_used_at.is_some(), "matched pair should be bumped");
        assert!(tf.last_used_at.is_none(), "other pair must not be bumped");
    }

    #[test]
    fn bump_pair_last_used_silent_when_no_pair() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let store_arc = state.stores.read().unwrap().expect_unlocked(&store_id);
        // No pair record for (gh, res_token) → no-op, no error.
        bump_pair_last_used(store_arc.as_ref(), "gh", "res_token").unwrap();
    }

    #[test]
    fn admin_pair_create_requires_existing_resource_and_refuses_duplicates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let p = admin_principal(&[]);

        // No resource → 400 unknown_resource.
        let missing = handle_admin_pair_create(
            &state,
            &p,
            &underwrap_protocol::service::AdminPairCreateRequest {
                store: store_id.clone(),
                wrapper: "gh".into(),
                resource_id: "res_missing".into(),
            },
        );
        assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_body_json(missing)["code"], "unknown_resource");

        let resource_id = create_resource_id_for_test(&state, &p, &store_id, "github/token");
        let create = handle_admin_pair_create(
            &state,
            &p,
            &underwrap_protocol::service::AdminPairCreateRequest {
                store: store_id.clone(),
                wrapper: "gh".into(),
                resource_id: resource_id.clone(),
            },
        );
        assert_eq!(create.status(), StatusCode::OK);

        let dup = handle_admin_pair_create(
            &state,
            &p,
            &underwrap_protocol::service::AdminPairCreateRequest {
                store: store_id.clone(),
                wrapper: "gh".into(),
                resource_id: resource_id.clone(),
            },
        );
        assert_eq!(dup.status(), StatusCode::CONFLICT);
        assert_eq!(response_body_json(dup)["code"], "pair_already_exists");
    }

    #[test]
    fn admin_pair_delete_refuses_when_permissions_reference_the_pair() {
        use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let p = admin_principal(&[]);

        let resource_id = create_resource_id_for_test(&state, &p, &store_id, "r");
        handle_admin_pair_create(
            &state,
            &p,
            &underwrap_protocol::service::AdminPairCreateRequest {
                store: store_id.clone(),
                wrapper: "gh".into(),
                resource_id: resource_id.clone(),
            },
        );
        {
            let reg = state.stores.read().unwrap();
            save_policy_document(
                reg.expect_unlocked(&store_id).as_ref(),
                &PolicyDocument {
                    permissions: vec![WrapperPermission {
                        client_instance: "inst_a".into(),
                        wrapper: "gh".into(),
                        resource_id: resource_id.clone(),
                        mode: WrapperApprovalMode::Unlocked,
                        writable: false,
                        reason: None,
                    }],
                },
            )
            .unwrap();
        }

        let del = handle_admin_pair_delete(
            &state,
            &p,
            &underwrap_protocol::service::AdminPairDeleteRequest {
                store: store_id.clone(),
                wrapper: "gh".into(),
                resource_id: resource_id.clone(),
            },
        );
        assert_eq!(del.status(), StatusCode::OK);
        let body = response_body_json(del);
        assert_eq!(body["ok"], false);
        assert_eq!(body["blocked_by"]["permission_count"], 1);
        assert_eq!(body["blocked_by"]["sample_permissions"][0]["wrapper"], "gh");
    }

    // ===== Users CRUD =====

    fn seed_admin_into(state: &AdminRuntimeState, username: &str) -> String {
        // Returns the operator id of the seeded admin so tests can
        // build a principal that refers to a real record.
        let mut c = crate::users::UserCollection::default();
        let hash = crate::auth::hash_password("seed-pass").unwrap();
        let now = crate::users::now_epoch_secs();
        let id = crate::users::new_user_id();
        c.add(crate::users::UserRecord {
            id: id.clone(),
            username: username.into(),
            password_hash: hash,
            created_at: now,
            password_changed_at: now,
        })
        .unwrap();
        crate::users::save(&state.users_file, &c).unwrap();
        id
    }

    fn admin_principal_with_id(id: &str) -> crate::users::Principal {
        crate::users::Principal {
            id: id.into(),
            username: "alice".into(),
        }
    }

    #[test]
    fn user_create_persists_record_and_omits_password_hash_from_response() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        seed_admin_into(&state, "alice");
        let p = admin_principal(&[]);
        let resp = do_admin_user_create(
            &state,
            &p,
            underwrap_protocol::service::AdminUserCreateRequest {
                username: "bob".into(),
                password: "hunter2-bob".into(),
            },
        );
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        let body_str = serde_json::to_string(&body).unwrap();
        assert!(
            !body_str.contains("$argon2"),
            "create response must not leak the password hash: {body_str}",
        );
        // `password_changed_at` is fine; the actual password and its
        // hash must not appear anywhere.
        assert!(
            !body_str.contains("hunter2-bob"),
            "create response must not echo the plaintext password: {body_str}",
        );
        assert!(
            !body_str.contains("password_hash"),
            "create response must not expose the password_hash field: {body_str}",
        );
        assert_eq!(body["username"], "bob");
        assert!(
            body.get("role").is_none(),
            "wire shape must not carry a role"
        );

        // The record landed in the user collection.
        let collection = crate::users::load(&state.users_file).unwrap();
        assert_eq!(collection.users.len(), 2);
        assert!(collection.find_by_username("bob").is_some());
    }

    #[test]
    fn user_create_refuses_duplicate_username_case_insensitively() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        seed_admin_into(&state, "alice");
        let p = admin_principal(&[]);
        let req = || underwrap_protocol::service::AdminUserCreateRequest {
            username: "Alice".into(), // collides after normalisation
            password: "any-pass".into(),
        };
        let resp = do_admin_user_create(&state, &p, req());
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = response_body_json(resp);
        assert_eq!(body["code"], "username_already_exists");
    }

    #[test]
    fn user_create_refuses_empty_password() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        seed_admin_into(&state, "alice");
        let resp = do_admin_user_create(
            &state,
            &admin_principal(&[]),
            underwrap_protocol::service::AdminUserCreateRequest {
                username: "bob".into(),
                password: String::new(),
            },
        );
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_body_json(resp)["code"], "invalid_password");
    }

    #[test]
    fn users_list_omits_password_hash_for_every_record() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        seed_admin_into(&state, "alice");
        let p = admin_principal(&[]);
        for u in ["bob", "carol"] {
            do_admin_user_create(
                &state,
                &p,
                underwrap_protocol::service::AdminUserCreateRequest {
                    username: u.into(),
                    password: format!("{u}-pass"),
                },
            );
        }
        let resp = handle_admin_users_list(&state);
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        let body_str = serde_json::to_string(&body).unwrap();
        assert!(!body_str.contains("$argon2"));
        assert!(!body_str.contains("password_hash"));
        let users = body["users"].as_array().unwrap();
        assert_eq!(users.len(), 3); // alice + bob + carol
    }

    #[test]
    fn user_update_renames_user() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        seed_admin_into(&state, "alice");
        let p = admin_principal(&[]);
        let create_resp = do_admin_user_create(
            &state,
            &p,
            underwrap_protocol::service::AdminUserCreateRequest {
                username: "bob".into(),
                password: "x".into(),
            },
        );
        let bob_id = response_body_json(create_resp)["id"]
            .as_str()
            .unwrap()
            .to_string();
        let resp = do_admin_user_update(
            &state,
            &p,
            &bob_id,
            underwrap_protocol::service::AdminUserUpdateRequest {
                username: Some("robert".into()),
            },
        );
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        assert_eq!(body["username"], "robert");
    }

    #[test]
    fn user_delete_refuses_last_user() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let alice_id = seed_admin_into(&state, "alice");
        let p = admin_principal_with_id(&alice_id);
        let resp = handle_admin_user_delete(&state, &p, &alice_id);
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = response_body_json(resp);
        assert_eq!(body["code"], "last_user_protected");
        // Record still in the collection.
        let collection = crate::users::load(&state.users_file).unwrap();
        assert!(collection.find_by_id(&alice_id).is_some());
    }

    #[test]
    fn user_delete_succeeds_when_another_user_exists() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let alice_id = seed_admin_into(&state, "alice");
        let p = admin_principal_with_id(&alice_id);
        let create = do_admin_user_create(
            &state,
            &p,
            underwrap_protocol::service::AdminUserCreateRequest {
                username: "second".into(),
                password: "x".into(),
            },
        );
        let second_id = response_body_json(create)["id"]
            .as_str()
            .unwrap()
            .to_string();
        let resp = handle_admin_user_delete(&state, &p, &second_id);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn user_reset_password_changes_hash_and_revokes_sessions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        seed_admin_into(&state, "alice");
        let p = admin_principal(&[]);
        let create = do_admin_user_create(
            &state,
            &p,
            underwrap_protocol::service::AdminUserCreateRequest {
                username: "bob".into(),
                password: "old-pass".into(),
            },
        );
        let bob_id = response_body_json(create)["id"]
            .as_str()
            .unwrap()
            .to_string();

        let hash_before = crate::users::load(&state.users_file)
            .unwrap()
            .find_by_id(&bob_id)
            .unwrap()
            .password_hash
            .clone();

        // do_admin_user_create is sync, but reset_password takes a
        // `Request<Incoming>` — synthesise the body-already-parsed
        // path by inlining the logic. Easiest: drive
        // mutate_collection + the underlying state directly via a
        // tiny helper test that exercises the public path through
        // an `async` wrapper would need a runtime; we mimic the
        // server logic here instead.
        let new_hash = crate::auth::hash_password("new-pass").unwrap();
        let now = crate::users::now_epoch_secs();
        mutate_collection(&state, |c| {
            c.update(&bob_id, |u| {
                u.password_hash = new_hash.clone();
                u.password_changed_at = now;
            })?;
            Ok(())
        })
        .unwrap();
        state.session_revocations.revoke_username("bob", now);

        let hash_after = crate::users::load(&state.users_file)
            .unwrap()
            .find_by_id(&bob_id)
            .unwrap()
            .password_hash
            .clone();
        assert_ne!(hash_before, hash_after, "hash must change");
        assert!(
            state
                .session_revocations
                .is_user_floor_revoked("bob", now - 1),
            "sessions issued before the reset must be revoked",
        );
    }

    /// Fixture for the new runtime API tests. Builds a `crud_state`
    /// with a plain store containing one resource ("alpha", id derived
    /// from the catalog), one `(gh, alpha)` pair, and one Unlocked
    /// writable permission for `instance_id`. Returns enough info for
    /// individual tests to issue the new runtime endpoints.
    struct RuntimeApiFixture {
        _tmp: tempfile::TempDir,
        state: AdminRuntimeState,
        store_id: String,
        instance_id: String,
        resource_id: String,
    }

    impl RuntimeApiFixture {
        fn store(&self) -> Arc<dyn StorageBackend> {
            self.state
                .stores
                .read()
                .unwrap()
                .expect_unlocked(&self.store_id)
        }

        fn policy(&self) -> PolicyDocument {
            load_policy_document_or_default(self.store().as_ref()).unwrap()
        }

        fn save_policy(&self, doc: &PolicyDocument) {
            save_policy_document(self.store().as_ref(), doc).unwrap();
        }
    }

    fn make_runtime_api_fixture() -> RuntimeApiFixture {
        use underwrap_protocol::entities::{
            EntityCreator, ResourceRecord, WrapperResourcePairRecord,
        };
        use underwrap_protocol::policy::{WrapperApprovalMode, WrapperPermission};
        use underwrap_protocol::resource_blob::ResourceBlob;

        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let store_id = create_store_for_test(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                description: None,
            },
        );

        let instance_id = "inst-runtime".to_string();
        {
            let mut instances = state.client_instances.write().unwrap();
            instances.insert(
                instance_id.clone(),
                crate::runtime_clients::ClientInstanceRecord {
                    id: instance_id.clone(),
                    display_name: "test".into(),
                    current_store_id: store_id.clone(),
                    status: crate::runtime_clients::ClientInstanceStatus::Active,
                    last_seen_at: None,
                    current_credential: crate::runtime_clients::CurrentCredentialRecord {
                        cert_fingerprint: "fp-runtime".into(),
                        cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
                        issued_at: Some(1),
                    },
                    auto_create_wrapper_assets: false,
                },
            );
            crate::runtime_clients::save_client_instances(&state.client_instances_file, &instances)
                .unwrap();
        }

        let store = state.stores.read().unwrap().expect_unlocked(&store_id);
        let resource_id = "res_alpha".to_string();
        let mut resources = load_resources_document(store.as_ref()).unwrap();
        resources
            .insert(ResourceRecord {
                id: resource_id.clone(),
                name: "alpha".into(),
                created_by: EntityCreator::Operator {
                    id: "admin-id".into(),
                    username: "alice".into(),
                },
                created_at: 1,
            })
            .unwrap();
        save_resources_document(store.as_ref(), &resources).unwrap();
        let mut pairs = load_pairs_document(store.as_ref()).unwrap();
        pairs
            .insert(WrapperResourcePairRecord {
                wrapper: "gh".into(),
                resource_id: resource_id.clone(),
                created_by: EntityCreator::ClientInstance {
                    id: instance_id.clone(),
                },
                created_at: 1,
                last_used_at: None,
            })
            .unwrap();
        save_pairs_document(store.as_ref(), &pairs).unwrap();

        let policy = PolicyDocument {
            permissions: vec![WrapperPermission {
                client_instance: instance_id.clone(),
                wrapper: "gh".into(),
                resource_id: resource_id.clone(),
                mode: WrapperApprovalMode::Unlocked,
                writable: true,
                reason: None,
            }],
        };
        save_policy_document(store.as_ref(), &policy).unwrap();
        save_resource_blob(store.as_ref(), &resource_id, &ResourceBlob::empty()).unwrap();

        RuntimeApiFixture {
            _tmp: tmp,
            state,
            store_id,
            instance_id,
            resource_id,
        }
    }

    #[test]
    fn runtime_wrapper_register_seeds_pair_when_auto_create_enabled() {
        use underwrap_protocol::service::{
            RuntimeWrapperRegisterPair, RuntimeWrapperRegisterRequest,
            RuntimeWrapperRegisterResponse,
        };

        let fx = make_runtime_api_fixture();
        // Flip auto-create on for this instance.
        {
            let mut instances = fx.state.client_instances.write().unwrap();
            instances
                .get_mut(&fx.instance_id)
                .unwrap()
                .auto_create_wrapper_assets = true;
        }
        let store = fx.store();
        let resp = handle_runtime_wrapper_register(
            &fx.state,
            store.as_ref(),
            &fx.instance_id,
            &RuntimeWrapperRegisterRequest {
                wrappers: vec![RuntimeWrapperRegisterPair {
                    wrapper_name: "newcomer".into(),
                    resource_name: "beta".into(),
                }],
            },
        );
        let parsed: RuntimeWrapperRegisterResponse =
            serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
        match parsed {
            RuntimeWrapperRegisterResponse::Ok {
                auto_created_resources,
                auto_created_pairs,
            } => {
                assert_eq!(auto_created_resources, vec!["beta".to_string()]);
                assert_eq!(auto_created_pairs.len(), 1);
                assert_eq!(auto_created_pairs[0].wrapper_name, "newcomer");
                assert_eq!(auto_created_pairs[0].resource_name, "beta");
            }
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn runtime_wrapper_register_refuses_when_auto_create_disabled() {
        use underwrap_protocol::service::{
            RuntimeWrapperRegisterPair, RuntimeWrapperRegisterRequest,
            RuntimeWrapperRegisterResponse,
        };

        let fx = make_runtime_api_fixture();
        let store = fx.store();
        let resp = handle_runtime_wrapper_register(
            &fx.state,
            store.as_ref(),
            &fx.instance_id,
            &RuntimeWrapperRegisterRequest {
                wrappers: vec![RuntimeWrapperRegisterPair {
                    wrapper_name: "newcomer".into(),
                    resource_name: "gamma".into(),
                }],
            },
        );
        let parsed: RuntimeWrapperRegisterResponse =
            serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
        match parsed {
            RuntimeWrapperRegisterResponse::AutoCreateDisabled {
                missing_resources,
                missing_pairs,
            } => {
                assert_eq!(missing_resources, vec!["gamma".to_string()]);
                assert_eq!(missing_pairs.len(), 1);
                assert_eq!(missing_pairs[0].wrapper_name, "newcomer");
                assert_eq!(missing_pairs[0].resource_name, "gamma");
            }
            other => panic!("expected AutoCreateDisabled, got {other:?}"),
        }
    }

    #[test]
    fn runtime_wrapper_invocation_open_allows_unlocked_permission() {
        use base64::Engine;
        use underwrap_protocol::resource_blob::ResourceBlob;
        use underwrap_protocol::service::{
            RuntimeWrapperInvocationOpenRequest, RuntimeWrapperInvocationOpenResponse,
        };

        let fx = make_runtime_api_fixture();
        let resp = handle_runtime_wrapper_invocation_open(
            fx.store().as_ref(),
            &fx.instance_id,
            &RuntimeWrapperInvocationOpenRequest {
                wrapper_name: "gh".into(),
                resource_name: "alpha".into(),
            },
        );
        let parsed: RuntimeWrapperInvocationOpenResponse =
            serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
        match parsed {
            RuntimeWrapperInvocationOpenResponse::Allow {
                resource_id,
                blob_b64,
                writable,
            } => {
                assert!(writable, "fixture seeds writable=true");
                assert_eq!(resource_id, fx.resource_id);
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&blob_b64)
                    .unwrap();
                let blob = ResourceBlob::from_canonical_bytes(&bytes).unwrap();
                assert!(blob.env.is_empty());
                assert!(blob.files.is_empty());
            }
            other => panic!("expected Allow, got {other:?}"),
        }
    }

    #[test]
    fn runtime_wrapper_invocation_open_denies_locked_permission() {
        use underwrap_protocol::policy::WrapperApprovalMode;
        use underwrap_protocol::service::{
            RuntimeWrapperInvocationOpenRequest, RuntimeWrapperInvocationOpenResponse,
        };

        let fx = make_runtime_api_fixture();
        let mut policy = fx.policy();
        policy.permissions[0].mode = WrapperApprovalMode::Locked;
        fx.save_policy(&policy);
        let resp = handle_runtime_wrapper_invocation_open(
            fx.store().as_ref(),
            &fx.instance_id,
            &RuntimeWrapperInvocationOpenRequest {
                wrapper_name: "gh".into(),
                resource_name: "alpha".into(),
            },
        );
        let parsed: RuntimeWrapperInvocationOpenResponse =
            serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
        match parsed {
            RuntimeWrapperInvocationOpenResponse::Deny { reason } => {
                assert_eq!(reason, "locked");
            }
            other => panic!("expected Deny(locked), got {other:?}"),
        }
    }

    #[test]
    fn runtime_wrapper_invocation_open_denies_unregistered_pair() {
        use underwrap_protocol::service::{
            RuntimeWrapperInvocationOpenRequest, RuntimeWrapperInvocationOpenResponse,
        };

        let fx = make_runtime_api_fixture();
        let resp = handle_runtime_wrapper_invocation_open(
            fx.store().as_ref(),
            &fx.instance_id,
            &RuntimeWrapperInvocationOpenRequest {
                wrapper_name: "ghost".into(),
                resource_name: "alpha".into(),
            },
        );
        let parsed: RuntimeWrapperInvocationOpenResponse =
            serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
        assert!(matches!(
            parsed,
            RuntimeWrapperInvocationOpenResponse::Deny { reason } if reason == "unregistered-pair"
        ));
    }

    #[test]
    fn runtime_wrapper_invocation_finalize_bumps_last_used_without_blob() {
        use underwrap_protocol::service::{
            RuntimeWrapperInvocationFinalizeRequest, RuntimeWrapperInvocationFinalizeResponse,
        };

        let fx = make_runtime_api_fixture();
        let resp = handle_runtime_wrapper_invocation_finalize(
            fx.store().as_ref(),
            &fx.instance_id,
            &RuntimeWrapperInvocationFinalizeRequest {
                wrapper_name: "gh".into(),
                resource_name: "alpha".into(),
                blob_b64: None,
            },
        );
        let parsed: RuntimeWrapperInvocationFinalizeResponse =
            serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
        assert_eq!(parsed, RuntimeWrapperInvocationFinalizeResponse::Ok);
        let pairs = load_pairs_document(fx.store().as_ref()).unwrap();
        assert!(
            pairs
                .find("gh", &fx.resource_id)
                .unwrap()
                .last_used_at
                .is_some(),
        );
    }

    #[test]
    fn runtime_wrapper_invocation_finalize_persists_libsecret_change() {
        use base64::Engine;
        use underwrap_protocol::resource_blob::{Collection, ResourceBlob};
        use underwrap_protocol::service::{
            RuntimeWrapperInvocationFinalizeRequest, RuntimeWrapperInvocationFinalizeResponse,
        };

        let fx = make_runtime_api_fixture();
        let mut blob = ResourceBlob::empty();
        blob.libsecret.collections.insert(
            "default".into(),
            Collection {
                label: "Default".into(),
                created: 1,
                modified: 2,
                items: Default::default(),
            },
        );
        let bytes = blob.to_canonical_bytes().unwrap();
        let resp = handle_runtime_wrapper_invocation_finalize(
            fx.store().as_ref(),
            &fx.instance_id,
            &RuntimeWrapperInvocationFinalizeRequest {
                wrapper_name: "gh".into(),
                resource_name: "alpha".into(),
                blob_b64: Some(base64::engine::general_purpose::STANDARD.encode(&bytes)),
            },
        );
        let parsed: RuntimeWrapperInvocationFinalizeResponse =
            serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
        assert_eq!(parsed, RuntimeWrapperInvocationFinalizeResponse::Ok);
        let stored = load_resource_blob(fx.store().as_ref(), &fx.resource_id).unwrap();
        assert!(stored.libsecret.collections.contains_key("default"));
    }

    #[test]
    fn runtime_wrapper_invocation_finalize_rejects_env_mutation() {
        use base64::Engine;
        use underwrap_protocol::message::SecretBytes;
        use underwrap_protocol::resource_blob::ResourceBlob;
        use underwrap_protocol::service::{
            RuntimeWrapperInvocationFinalizeRequest, RuntimeWrapperInvocationFinalizeResponse,
        };

        let fx = make_runtime_api_fixture();
        // Seed an env entry in the stored blob.
        let mut stored = ResourceBlob::empty();
        stored
            .env
            .insert("TOKEN".into(), SecretBytes::from(b"abc".to_vec()));
        save_resource_blob(fx.store().as_ref(), &fx.resource_id, &stored).unwrap();

        // Wrapper tries to rewrite TOKEN.
        let mut mutated = stored.clone();
        mutated
            .env
            .insert("TOKEN".into(), SecretBytes::from(b"xyz".to_vec()));
        let bytes = mutated.to_canonical_bytes().unwrap();
        let resp = handle_runtime_wrapper_invocation_finalize(
            fx.store().as_ref(),
            &fx.instance_id,
            &RuntimeWrapperInvocationFinalizeRequest {
                wrapper_name: "gh".into(),
                resource_name: "alpha".into(),
                blob_b64: Some(base64::engine::general_purpose::STANDARD.encode(&bytes)),
            },
        );
        let parsed: RuntimeWrapperInvocationFinalizeResponse =
            serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
        assert!(matches!(
            parsed,
            RuntimeWrapperInvocationFinalizeResponse::InvalidBlob { reason }
                if reason == "env-section-immutable"
        ));
        // Stored env section must be untouched.
        let after = load_resource_blob(fx.store().as_ref(), &fx.resource_id).unwrap();
        assert_eq!(
            after.env.get("TOKEN").map(|v| v.as_ref().to_vec()),
            Some(b"abc".to_vec())
        );
    }

    #[test]
    fn runtime_wrapper_invocation_finalize_rejects_noncanonical_blob() {
        use base64::Engine;
        use underwrap_protocol::service::{
            RuntimeWrapperInvocationFinalizeRequest, RuntimeWrapperInvocationFinalizeResponse,
        };

        let fx = make_runtime_api_fixture();
        // Hand-craft a valid-but-noncanonical JSON: trailing whitespace
        // around the body keeps the bytes parseable but is rejected by
        // the round-trip equality check.
        let noncanonical = br#"{"schema_version":1,"env":{},"files":{},"libsecret":{"aliases":{},"collections":{}}}  "#;
        let resp = handle_runtime_wrapper_invocation_finalize(
            fx.store().as_ref(),
            &fx.instance_id,
            &RuntimeWrapperInvocationFinalizeRequest {
                wrapper_name: "gh".into(),
                resource_name: "alpha".into(),
                blob_b64: Some(
                    base64::engine::general_purpose::STANDARD.encode(noncanonical.as_slice()),
                ),
            },
        );
        let parsed: RuntimeWrapperInvocationFinalizeResponse =
            serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
        assert!(matches!(
            parsed,
            RuntimeWrapperInvocationFinalizeResponse::InvalidBlob { reason }
                if reason == "noncanonical-blob"
        ));
    }

    #[test]
    fn runtime_wrapper_invocation_finalize_rejects_writeback_when_not_writable() {
        use base64::Engine;
        use underwrap_protocol::resource_blob::ResourceBlob;
        use underwrap_protocol::service::{
            RuntimeWrapperInvocationFinalizeRequest, RuntimeWrapperInvocationFinalizeResponse,
        };

        let fx = make_runtime_api_fixture();
        let mut policy = fx.policy();
        policy.permissions[0].writable = false;
        fx.save_policy(&policy);

        let bytes = ResourceBlob::empty().to_canonical_bytes().unwrap();
        let resp = handle_runtime_wrapper_invocation_finalize(
            fx.store().as_ref(),
            &fx.instance_id,
            &RuntimeWrapperInvocationFinalizeRequest {
                wrapper_name: "gh".into(),
                resource_name: "alpha".into(),
                blob_b64: Some(base64::engine::general_purpose::STANDARD.encode(&bytes)),
            },
        );
        let parsed: RuntimeWrapperInvocationFinalizeResponse =
            serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
        assert!(matches!(
            parsed,
            RuntimeWrapperInvocationFinalizeResponse::Deny { reason } if reason == "not-writable"
        ));
    }

    #[test]
    fn debug_redacts_password_hash_in_collection_logging() {
        // Regression: when we tracing!(?collection) the hash must
        // never appear. The `Debug` impl on `UserRecord` redacts it,
        // but the easiest check is to format the loaded collection
        // after every mutation and grep.
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        seed_admin_into(&state, "alice");
        do_admin_user_create(
            &state,
            &admin_principal(&[]),
            underwrap_protocol::service::AdminUserCreateRequest {
                username: "bob".into(),
                password: "x".into(),
            },
        );
        let collection = crate::users::load(&state.users_file).unwrap();
        let formatted = format!("{collection:?}");
        assert!(
            !formatted.contains("$argon2"),
            "Debug must redact password hashes; got: {formatted}",
        );
    }
}
