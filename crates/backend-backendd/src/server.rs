//! Backend daemon server.
//!
//! Serves the runtime HTTPS API for `underwrapd` clients and a separate admin
//! HTTP API for operators and tests.

use anyhow::{Context, Result};
use base64::Engine;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio::time::{Duration, interval};
use tokio_rustls::TlsAcceptor;
use tracing::{error, info, warn};

use underwrap_protocol::config::StorageConfig;
use underwrap_protocol::policy::{
    POLICY_DOCUMENT_SECRET_ID, PolicyDecision, PolicyDocument, WrapperApprovalMode,
};

use underwrap_store::StorageBackend;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceTlsConfig {
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
    pub client_ca_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceClientConfig {
    pub store: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub listen: String,
    pub tls: ServiceTlsConfig,
    #[serde(default)]
    pub clients: HashMap<String, ServiceClientConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminTokenConfig {
    /// Path to a file whose contents are the opaque bearer token.
    pub token_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminTlsConfig {
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminConfig {
    pub listen: String,
    /// Bearer tokens by operator name. Each request must present one via
    /// `Authorization: Bearer <token>`. When empty, the admin API is
    /// unavailable (no requests can authenticate).
    #[serde(default)]
    pub tokens: HashMap<String, AdminTokenConfig>,
    /// Optional TLS termination for the admin listener. When absent, the
    /// listener serves plain HTTP and the listen address must be loopback.
    #[serde(default)]
    pub tls: Option<AdminTlsConfig>,
    /// Optional allowlist of `Origin` (or `Referer`) values for
    /// state-changing requests. When set, every non-GET/HEAD/OPTIONS
    /// request must carry an Origin matching one of these entries.
    /// Foundation for the CSRF work in ticket 212.
    #[serde(default)]
    pub allowed_origins: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    pub stores: HashMap<String, StorageConfig>,
    #[serde(default)]
    pub service: Option<ServiceConfig>,
    #[serde(default)]
    pub admin: Option<AdminConfig>,
}

struct ServiceRuntimeState {
    clients: HashMap<String, Arc<dyn StorageBackend>>,
}

struct AdminRuntimeState {
    stores: HashMap<String, Arc<dyn StorageBackend>>,
    /// Operator name → opaque bearer token. Both fields are trusted; the
    /// caller authenticates by presenting a matching token.
    tokens: HashMap<String, String>,
    /// When present, every state-changing request must carry an `Origin`
    /// (or `Referer`) matching one of these entries.
    allowed_origins: Option<Vec<String>>,
}

const CATALOG_DOCUMENT_SECRET_ID: &str = "__underwrap/catalog/document";
const AUDIT_HISTORY_SECRET_ID: &str = "__underwrap/admin/ui/audit-history";
const ADMIN_UI_AUDIT_MAX_EVENTS: usize = 2000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredCatalogEntry {
    principal: String,
    host: String,
    user: String,
    instance: String,
    wrapper: String,
    resource: String,
}

pub async fn run(config: BackendConfig, shutdown: Option<Arc<Notify>>) -> Result<()> {
    let stores = open_stores(&config.stores)?;

    let mut tasks = JoinSet::new();

    if let Some(service_config) = &config.service {
        let service_listener = TcpListener::bind(&service_config.listen)
            .await
            .with_context(|| format!("binding service listener {}", service_config.listen))?;
        let service_state = Arc::new(build_service_runtime_state(&stores, service_config)?);
        let tls_acceptor = build_tls_acceptor(&service_config.tls)?;
        info!(listen = %service_config.listen, clients = service_config.clients.len(), "backend HTTPS service listening");
        tasks.spawn(service_accept_loop(
            service_listener,
            tls_acceptor,
            service_state,
        ));
    }

    if let Some(admin_config) = &config.admin {
        validate_admin_config(admin_config)?;
        let admin_listener = TcpListener::bind(&admin_config.listen)
            .await
            .with_context(|| format!("binding admin listener {}", admin_config.listen))?;
        let tokens = load_admin_tokens(&admin_config.tokens)?;
        let tls_acceptor = match &admin_config.tls {
            Some(tls) => Some(build_admin_tls_acceptor(tls)?),
            None => None,
        };
        let admin_state = Arc::new(AdminRuntimeState {
            stores: stores.clone(),
            tokens,
            allowed_origins: admin_config.allowed_origins.clone(),
        });
        info!(
            listen = %admin_config.listen,
            stores = admin_state.stores.len(),
            operators = admin_state.tokens.len(),
            tls = tls_acceptor.is_some(),
            "backend admin listening",
        );
        tasks.spawn(admin_accept_loop(admin_listener, admin_state, tls_acceptor));
    }

    tasks.spawn(expiry_sweeper_loop(stores.clone()));

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

fn open_stores(
    configs: &HashMap<String, StorageConfig>,
) -> Result<HashMap<String, Arc<dyn StorageBackend>>> {
    let mut stores = HashMap::new();
    for (name, config) in configs {
        let store: Arc<dyn StorageBackend> = Arc::from(underwrap_store::open(config)?);
        stores.insert(name.clone(), store);
    }
    Ok(stores)
}

fn build_service_runtime_state(
    stores: &HashMap<String, Arc<dyn StorageBackend>>,
    config: &ServiceConfig,
) -> Result<ServiceRuntimeState> {
    let mut clients = HashMap::new();
    for (principal, client) in &config.clients {
        let store = stores
            .get(&client.store)
            .with_context(|| {
                format!(
                    "service client '{principal}' references unknown store '{}'",
                    client.store
                )
            })?
            .clone();
        clients.insert(principal.clone(), store);
    }
    Ok(ServiceRuntimeState { clients })
}

fn build_tls_acceptor(config: &ServiceTlsConfig) -> Result<TlsAcceptor> {
    use rustls::server::WebPkiClientVerifier;
    use rustls::{RootCertStore, ServerConfig};

    let _ = rustls::crypto::ring::default_provider().install_default();

    let certs = load_cert_chain(&config.cert_file)?;
    let key = load_private_key(&config.key_file)?;
    let client_ca_certs = load_cert_chain(&config.client_ca_file)?;

    let mut roots = RootCertStore::empty();
    for cert in client_ca_certs {
        roots.add(cert).context("adding client CA certificate")?;
    }

    let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
        .build()
        .context("building rustls client verifier")?;

    let server_config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(certs, key)
        .context("building rustls server config")?;

    Ok(TlsAcceptor::from(Arc::new(server_config)))
}

fn build_admin_tls_acceptor(config: &AdminTlsConfig) -> Result<TlsAcceptor> {
    use rustls::ServerConfig;

    let _ = rustls::crypto::ring::default_provider().install_default();

    let certs = load_cert_chain(&config.cert_file)?;
    let key = load_private_key(&config.key_file)?;

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("building rustls admin server config")?;

    Ok(TlsAcceptor::from(Arc::new(server_config)))
}

/// Validate an `AdminConfig` at startup. The admin listener carries
/// browser traffic, so we refuse to expose it as plain HTTP on a
/// non-loopback address; either TLS must be configured or the listen
/// address must resolve to loopback.
fn validate_admin_config(config: &AdminConfig) -> Result<()> {
    if config.tls.is_none() && !is_loopback_listen(&config.listen) {
        anyhow::bail!(
            "admin listener {} is not loopback and TLS is not configured; \
             refusing to expose plain HTTP on a non-loopback address",
            config.listen
        );
    }
    if let Some(origins) = &config.allowed_origins {
        for origin in origins {
            if origin.trim().is_empty() {
                anyhow::bail!("admin.allowed_origins contains an empty entry");
            }
        }
    }
    Ok(())
}

/// Return true if the host portion of a `host:port` (or `[ipv6]:port`)
/// listen address is a loopback IP literal or `localhost`. Hostnames
/// other than `localhost` are not resolved here and are treated as
/// non-loopback to keep the check deterministic.
fn is_loopback_listen(listen: &str) -> bool {
    let host = match listen.rsplit_once(':') {
        Some((host, _)) => host,
        None => return false,
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.is_empty() {
        return false;
    }
    if let Ok(addr) = host.parse::<std::net::IpAddr>() {
        return addr.is_loopback();
    }
    host.eq_ignore_ascii_case("localhost")
}

/// Check the `Origin` (or `Referer`) header of a state-changing request
/// against the configured allowlist. GET/HEAD/OPTIONS are not gated.
/// Returns `Ok(())` when no allowlist is configured.
fn check_admin_origin(
    method: &Method,
    headers: &hyper::HeaderMap,
    allowed: Option<&[String]>,
) -> std::result::Result<(), &'static str> {
    if matches!(method, &Method::GET | &Method::HEAD | &Method::OPTIONS) {
        return Ok(());
    }
    let allowed = match allowed {
        Some(list) if !list.is_empty() => list,
        _ => return Ok(()),
    };
    let presented = headers
        .get(hyper::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            headers
                .get(hyper::header::REFERER)
                .and_then(|v| v.to_str().ok())
                .and_then(referer_to_origin)
        });
    let presented = match presented {
        Some(value) => value,
        None => return Err("missing Origin/Referer on state-changing request"),
    };
    if allowed.iter().any(|a| a.trim() == presented) {
        Ok(())
    } else {
        Err("Origin/Referer not in allowed_origins")
    }
}

/// Strip path/query from a `Referer` value, returning the scheme+host[:port]
/// prefix. Returns `None` for malformed inputs so callers can reject.
fn referer_to_origin(referer: &str) -> Option<String> {
    let trimmed = referer.trim();
    let (scheme, rest) = trimmed.split_once("://")?;
    if scheme.is_empty() {
        return None;
    }
    let authority = match rest.find('/') {
        Some(idx) => &rest[..idx],
        None => rest,
    };
    if authority.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{authority}"))
}

fn load_cert_chain(path: &Path) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    let pem = std::fs::read(path)
        .with_context(|| format!("reading certificate file {}", path.display()))?;
    let mut reader = std::io::BufReader::new(pem.as_slice());
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("parsing certificate PEM")?;
    anyhow::ensure!(
        !certs.is_empty(),
        "certificate file '{}' contained no certificates",
        path.display()
    );
    Ok(certs)
}

fn load_private_key(path: &Path) -> Result<rustls::pki_types::PrivateKeyDer<'static>> {
    let pem = std::fs::read(path)
        .with_context(|| format!("reading private key file {}", path.display()))?;
    let mut reader = std::io::BufReader::new(pem.as_slice());
    if let Some(key) =
        rustls_pemfile::private_key(&mut reader).context("parsing private key PEM")?
    {
        Ok(key)
    } else {
        anyhow::bail!(
            "private key file '{}' contained no private key",
            path.display()
        )
    }
}

fn extract_client_principal(
    certs: &[rustls::pki_types::CertificateDer<'static>],
) -> Result<String> {
    let cert = certs
        .first()
        .context("client did not present a certificate")?;
    let (_, parsed) =
        x509_parser::parse_x509_certificate(cert.as_ref()).context("parsing client certificate")?;
    let common_name = parsed
        .subject()
        .iter_common_name()
        .next()
        .context("client certificate missing common name")?
        .as_str()
        .context("client certificate common name is not valid utf-8")?;
    Ok(common_name.to_string())
}

#[cfg(test)]
fn get_policy_document(
    stores: &HashMap<String, Arc<dyn StorageBackend>>,
    store_name: &str,
) -> Result<Option<String>> {
    let store = stores
        .get(store_name)
        .with_context(|| format!("unknown store '{store_name}'"))?;
    let document = store.read(POLICY_DOCUMENT_SECRET_ID)?;
    Ok(document.map(|bytes| String::from_utf8_lossy(bytes.as_ref()).into_owned()))
}

#[cfg(test)]
fn put_policy_document(
    stores: &HashMap<String, Arc<dyn StorageBackend>>,
    store_name: &str,
    document_json: &str,
) -> Result<()> {
    let store = stores
        .get(store_name)
        .with_context(|| format!("unknown store '{store_name}'"))?;
    let _: underwrap_protocol::policy::PolicyDocument =
        serde_json::from_str(document_json).context("parsing policy document json")?;
    store.write(POLICY_DOCUMENT_SECRET_ID, document_json.as_bytes())
}

async fn admin_accept_loop(
    listener: TcpListener,
    state: Arc<AdminRuntimeState>,
    tls_acceptor: Option<TlsAcceptor>,
) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let state = Arc::clone(&state);
        let tls_acceptor = tls_acceptor.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_admin_connection(stream, state, tls_acceptor).await {
                error!("backend admin connection error: {e:#}");
            }
        });
    }
}

async fn handle_admin_connection(
    stream: tokio::net::TcpStream,
    state: Arc<AdminRuntimeState>,
    tls_acceptor: Option<TlsAcceptor>,
) -> Result<()> {
    let service = service_fn(move |req| {
        let state = Arc::clone(&state);
        async move { handle_admin_request(req, state).await }
    });

    let builder = hyper::server::conn::http1::Builder::new();
    match tls_acceptor {
        Some(acceptor) => {
            let tls_stream = acceptor
                .accept(stream)
                .await
                .context("accepting admin TLS connection")?;
            builder
                .serve_connection(TokioIo::new(tls_stream), service)
                .await
                .context("serving admin HTTPS connection")?;
        }
        None => {
            builder
                .serve_connection(TokioIo::new(stream), service)
                .await
                .context("serving admin HTTP connection")?;
        }
    }
    Ok(())
}

async fn service_accept_loop(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    state: Arc<ServiceRuntimeState>,
) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_service_connection(stream, acceptor, state).await {
                error!("backend service connection error: {e:#}");
            }
        });
    }
}

async fn handle_service_connection(
    stream: tokio::net::TcpStream,
    acceptor: TlsAcceptor,
    state: Arc<ServiceRuntimeState>,
) -> Result<()> {
    let tls_stream = acceptor
        .accept(stream)
        .await
        .context("accepting TLS connection")?;
    let principal = extract_client_principal(
        tls_stream
            .get_ref()
            .1
            .peer_certificates()
            .context("reading peer certificates")?,
    )?;
    let store = state
        .clients
        .get(&principal)
        .with_context(|| format!("unknown client principal '{principal}'"))?
        .clone();

    let service = service_fn(move |req| {
        let principal = principal.clone();
        let store = store.clone();
        async move { handle_service_request(req, principal, store).await }
    });

    hyper::server::conn::http1::Builder::new()
        .serve_connection(TokioIo::new(tls_stream), service)
        .await
        .context("serving HTTPS connection")?;
    Ok(())
}

async fn handle_service_request(
    request: Request<Incoming>,
    principal: String,
    store: Arc<dyn StorageBackend>,
) -> std::result::Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let response = match (request.method(), request.uri().path()) {
        (&Method::GET, "/v1/status") => json_response(
            StatusCode::OK,
            &underwrap_protocol::service::ServiceStatusResponse {
                ok: true,
                principal,
            },
        ),
        (&Method::POST, "/v1/read") => {
            match parse_json_body::<underwrap_protocol::service::ServiceReadRequest>(request).await
            {
                Ok(body) => match store.read(&body.secret_id) {
                    Ok(material) => json_response(
                        StatusCode::OK,
                        &underwrap_protocol::service::ServiceReadResponse {
                            material_b64: material.map(|material| {
                                base64::engine::general_purpose::STANDARD.encode(material.as_ref())
                            }),
                        },
                    ),
                    Err(e) => error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal",
                        &format!("storage error: {e}"),
                    ),
                },
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/write") => {
            match parse_json_body::<underwrap_protocol::service::ServiceWriteRequest>(request).await
            {
                Ok(body) => {
                    match base64::engine::general_purpose::STANDARD.decode(&body.material_b64) {
                        Ok(material) => match store.write(&body.secret_id, &material) {
                            Ok(()) => json_response(
                                StatusCode::OK,
                                &underwrap_protocol::service::ServiceWriteResponse { ok: true },
                            ),
                            Err(e) => error_response(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "internal",
                                &format!("storage error: {e}"),
                            ),
                        },
                        Err(e) => error_response(
                            StatusCode::BAD_REQUEST,
                            "invalid_request",
                            &format!("invalid base64 payload: {e}"),
                        ),
                    }
                }
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/list") => {
            match parse_json_body::<underwrap_protocol::service::ServiceListRequest>(request).await
            {
                Ok(body) => match store.list(&body.prefix) {
                    Ok(ids) => json_response(
                        StatusCode::OK,
                        &underwrap_protocol::service::ServiceListResponse { ids },
                    ),
                    Err(e) => error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal",
                        &format!("storage error: {e}"),
                    ),
                },
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/read-prefixes") => {
            match parse_json_body::<underwrap_protocol::service::ServiceReadPrefixesRequest>(
                request,
            )
            .await
            {
                Ok(body) => match store.read_prefixes(&body.prefixes) {
                    Ok(entries) => {
                        let mut entries: Vec<_> = entries
                            .into_iter()
                            .map(
                                |(id, material)| underwrap_protocol::service::ServiceBundleEntry {
                                    id,
                                    material_b64: base64::engine::general_purpose::STANDARD
                                        .encode(material.as_ref()),
                                },
                            )
                            .collect();
                        entries.sort_by(|a, b| a.id.cmp(&b.id));
                        json_response(
                            StatusCode::OK,
                            &underwrap_protocol::service::ServiceReadPrefixesResponse { entries },
                        )
                    }
                    Err(e) => error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal",
                        &format!("storage error: {e}"),
                    ),
                },
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/catalog/put") => match parse_json_body::<
            underwrap_protocol::service::ServiceCatalogPutRequest,
        >(request)
        .await
        {
            Ok(body) => {
                match replace_catalog_entries_for_principal(&*store, &principal, &body.entries) {
                    Ok(()) => json_response(
                        StatusCode::OK,
                        &underwrap_protocol::service::ServiceCatalogPutResponse { ok: true },
                    ),
                    Err(e) => error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal",
                        &format!("catalog storage error: {e}"),
                    ),
                }
            }
            Err(response) => response,
        },
        (&Method::POST, "/v1/delete") => {
            match parse_json_body::<underwrap_protocol::service::ServiceDeleteRequest>(request)
                .await
            {
                Ok(body) => match store.delete(&body.secret_id) {
                    Ok(existed) => json_response(
                        StatusCode::OK,
                        &underwrap_protocol::service::ServiceDeleteResponse { existed },
                    ),
                    Err(e) => error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal",
                        &format!("storage error: {e}"),
                    ),
                },
                Err(response) => response,
            }
        }
        _ => error_response(StatusCode::NOT_FOUND, "not_found", "unknown endpoint"),
    };
    Ok(response)
}

async fn parse_json_body<T: serde::de::DeserializeOwned>(
    request: Request<Incoming>,
) -> std::result::Result<T, Response<Full<Bytes>>> {
    let body = match request.into_body().collect().await {
        Ok(body) => body.to_bytes(),
        Err(e) => {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("failed to read request body: {e}"),
            ));
        }
    };
    serde_json::from_slice(&body).map_err(|e| {
        error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            &format!("invalid JSON request body: {e}"),
        )
    })
}

fn json_response<T: serde::Serialize>(status: StatusCode, body: &T) -> Response<Full<Bytes>> {
    let bytes = serde_json::to_vec(body).expect("serializing service response");
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(bytes)))
        .expect("building HTTP response")
}

const ADMIN_UI_MAX_FILE_BYTES: usize = 1024 * 1024;

fn error_response(status: StatusCode, code: &str, message: &str) -> Response<Full<Bytes>> {
    json_response(
        status,
        &underwrap_protocol::service::ServiceErrorResponse {
            code: code.to_string(),
            message: message.to_string(),
        },
    )
}

fn handle_admin_secret_write(
    state: &AdminRuntimeState,
    operator: &str,
    body: underwrap_protocol::service::AdminSecretWriteRequest,
) -> Response<Full<Bytes>> {
    if body.secret_id == POLICY_DOCUMENT_SECRET_ID {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "policy document is reserved; use /v1/admin/policy/put",
        );
    }
    let material = match base64::engine::general_purpose::STANDARD.decode(&body.material_b64) {
        Ok(material) => material,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("invalid base64 payload: {e}"),
            );
        }
    };

    let store = match admin_store(state, &body.store) {
        Ok(store) => store,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    };

    match store.write(&body.secret_id, &material) {
        Ok(()) => {
            info!(
                audit = "secret_stored",
                operator = %operator,
                store = %body.store,
                secret = %body.secret_id,
            );
            json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminOkResponse { ok: true },
            )
        }
        Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    }
}

fn load_catalog_document(store: &dyn StorageBackend) -> Result<Vec<StoredCatalogEntry>> {
    let Some(material) = store.read(CATALOG_DOCUMENT_SECRET_ID)? else {
        return Ok(Vec::new());
    };
    serde_json::from_slice(material.as_ref()).context("parsing backend catalog document")
}

fn load_policy_document(store: &dyn StorageBackend) -> Result<PolicyDocument> {
    match store.read(POLICY_DOCUMENT_SECRET_ID)? {
        Some(material) => {
            serde_json::from_slice(material.as_ref()).context("parsing backend policy document")
        }
        None => Ok(PolicyDocument::default()),
    }
}

fn load_audit_history(
    store: &dyn StorageBackend,
) -> Result<Vec<underwrap_protocol::service::AdminUiAuditEvent>> {
    match store.read(AUDIT_HISTORY_SECRET_ID)? {
        Some(material) => serde_json::from_slice(material.as_ref())
            .context("parsing backend audit history document"),
        None => Ok(Vec::new()),
    }
}

fn store_audit_history(
    store: &dyn StorageBackend,
    events: &[underwrap_protocol::service::AdminUiAuditEvent],
) -> Result<()> {
    store.write(
        AUDIT_HISTORY_SECRET_ID,
        serde_json::to_vec(events)?.as_slice(),
    )?;
    Ok(())
}

fn append_audit_history_event(
    store: &dyn StorageBackend,
    event: underwrap_protocol::service::AdminUiAuditEvent,
) -> Result<()> {
    let mut events = load_audit_history(store)?;
    events.push(event);
    if events.len() > ADMIN_UI_AUDIT_MAX_EVENTS {
        let remove = events.len() - ADMIN_UI_AUDIT_MAX_EVENTS;
        events.drain(..remove);
    }
    store_audit_history(store, &events)
}

fn admin_ui_audit_query(
    store: &dyn StorageBackend,
    request: &underwrap_protocol::service::AdminUiAuditQueryRequest,
) -> Result<underwrap_protocol::service::AdminUiAuditQueryResponse> {
    let mut events: Vec<_> = load_audit_history(store)?
        .into_iter()
        .filter(|event| {
            request
                .operator
                .as_deref()
                .is_none_or(|operator| event.operator == operator)
                && request
                    .wrapper
                    .as_deref()
                    .is_none_or(|wrapper| event.wrapper.as_deref() == Some(wrapper))
                && request
                    .resource
                    .as_deref()
                    .is_none_or(|resource| event.resource.as_deref() == Some(resource))
                && request
                    .action
                    .as_deref()
                    .is_none_or(|action| event.action == action)
                && request.from_epoch.is_none_or(|from| event.ts_epoch >= from)
                && request.to_epoch.is_none_or(|to| event.ts_epoch <= to)
        })
        .collect();
    events.sort_by(|a, b| b.ts_epoch.cmp(&a.ts_epoch).then(b.action.cmp(&a.action)));
    let total = events.len();
    let start = request.page.saturating_sub(1) * request.page_size.max(1);
    let items = events
        .into_iter()
        .skip(start)
        .take(request.page_size.max(1))
        .collect();
    Ok(underwrap_protocol::service::AdminUiAuditQueryResponse { items, total })
}

fn split_resource_id(secret_id: &str) -> Option<(&str, &str, &str)> {
    for slot in ["/env/", "/files/", "/libsecret/"] {
        if let Some((resource, rest)) = secret_id.split_once(slot) {
            return Some((resource, &slot[1..slot.len() - 1], rest));
        }
    }
    None
}

fn looks_like_utf8_text(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    text.chars()
        .all(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t'))
}

fn resource_query_matches(
    item: &underwrap_protocol::service::AdminUiResourceListItem,
    query: &str,
) -> bool {
    if query.is_empty() {
        return true;
    }
    let query = query.to_ascii_lowercase();
    item.resource.to_ascii_lowercase().contains(&query)
        || item
            .catalog_wrappers
            .iter()
            .any(|wrapper| wrapper.to_ascii_lowercase().contains(&query))
}

fn slot_filter_matches(
    item: &underwrap_protocol::service::AdminUiResourceListItem,
    slot: Option<&str>,
) -> Result<bool> {
    Ok(match slot {
        None | Some("any") => true,
        Some("env") => item.slots.env,
        Some("files") => item.slots.files,
        Some("libsecret") => item.slots.libsecret,
        Some(other) => anyhow::bail!("unknown slot filter '{other}'"),
    })
}

fn mutation_filter_matches(
    item: &underwrap_protocol::service::AdminUiResourceListItem,
    mutation_state: Option<&str>,
) -> Result<bool> {
    Ok(match mutation_state {
        None | Some("any") => true,
        Some("active") => item.mutation.active,
        Some("inactive") => !item.mutation.active,
        Some(other) => anyhow::bail!("unknown mutation_state filter '{other}'"),
    })
}

fn active_mutation_targets(doc: &PolicyDocument, now_epoch: u64) -> HashSet<(String, String)> {
    let mut out = HashSet::new();
    for rule in &doc.rules {
        if rule.operation == underwrap_protocol::policy::OP_STORE_WRITE
            && rule.effect == underwrap_protocol::policy::PolicyEffect::Allow
        {
            out.insert((rule.wrapper.clone(), rule.resource.clone()));
        }
    }
    for grant in &doc.grants {
        if grant.operation == underwrap_protocol::policy::OP_STORE_WRITE
            && grant.effect == underwrap_protocol::policy::PolicyEffect::Allow
        {
            let starts_ok = grant.starts_at_epoch.is_none_or(|start| now_epoch >= start);
            let expires_ok = grant.expires_at_epoch.is_none_or(|end| now_epoch < end);
            if starts_ok && expires_ok {
                out.insert((grant.wrapper.clone(), grant.resource.clone()));
            }
        }
    }
    out
}

fn admin_ui_overview_for_store(
    store_name: &str,
    store: &dyn StorageBackend,
    now_epoch: u64,
) -> Result<underwrap_protocol::service::AdminUiOverviewStore> {
    let secret_ids = store.list("")?;
    let catalog = load_catalog_document(store)?;
    let policy = load_policy_document(store)?;

    let resource_count = secret_ids
        .iter()
        .filter(|id| *id != POLICY_DOCUMENT_SECRET_ID && *id != CATALOG_DOCUMENT_SECRET_ID)
        .filter_map(|id| split_resource_id(id).map(|(resource, _, _)| resource.to_string()))
        .chain(catalog.iter().map(|entry| entry.resource.clone()))
        .collect::<BTreeSet<_>>()
        .len();

    let mut all_wrappers: BTreeSet<String> =
        catalog.iter().map(|entry| entry.wrapper.clone()).collect();
    all_wrappers.extend(policy.wrappers.iter().map(|entry| entry.wrapper.clone()));

    let mut wrapper_counts = underwrap_protocol::service::AdminUiWrapperCounts {
        locked: 0,
        unlocked: 0,
        unlocked_until: 0,
        per_call_approval: 0,
    };
    for wrapper in all_wrappers {
        match policy.wrapper_mode(&wrapper) {
            WrapperApprovalMode::Locked => wrapper_counts.locked += 1,
            WrapperApprovalMode::Unlocked {
                until_epoch: Some(_),
            } => wrapper_counts.unlocked_until += 1,
            WrapperApprovalMode::Unlocked { until_epoch: None } => wrapper_counts.unlocked += 1,
            WrapperApprovalMode::PerCallApproval { .. } => wrapper_counts.per_call_approval += 1,
        }
    }

    Ok(underwrap_protocol::service::AdminUiOverviewStore {
        store: store_name.to_string(),
        resource_count,
        wrapper_counts,
        active_mutation_windows: active_mutation_targets(&policy, now_epoch).len(),
        recent_audit_events: load_audit_history(store)?.len(),
    })
}

fn admin_ui_resource_detail(
    store: &dyn StorageBackend,
    resource: &str,
) -> Result<underwrap_protocol::service::AdminUiResourceGetResponse> {
    let secret_ids = store.list("")?;
    let catalog = load_catalog_document(store)?;
    let resource_known_in_catalog = catalog.iter().any(|entry| entry.resource == resource);
    let mut slots = underwrap_protocol::service::AdminUiResourceSlotsDetail::default();

    for secret_id in secret_ids {
        if secret_id == POLICY_DOCUMENT_SECRET_ID || secret_id == CATALOG_DOCUMENT_SECRET_ID {
            continue;
        }
        let Some((secret_resource, slot, entry)) = split_resource_id(&secret_id) else {
            continue;
        };
        if secret_resource != resource {
            continue;
        }
        let material = store
            .read(&secret_id)?
            .with_context(|| format!("secret id '{secret_id}' disappeared during detail load"))?;
        let material_ref = material.as_ref();
        let summary = underwrap_protocol::service::AdminUiResourceEntrySummary {
            entry: entry.to_string(),
            secret_id: secret_id.clone(),
            size_bytes: material_ref.len(),
            is_utf8: looks_like_utf8_text(material_ref),
            sha256_hex: (slot == "files").then(|| {
                use sha2::{Digest, Sha256};
                hex::encode(Sha256::digest(material_ref))
            }),
        };
        match slot {
            "env" => slots.env.push(summary),
            "files" => slots.files.push(summary),
            "libsecret" => slots.libsecret.push(summary),
            _ => {}
        }
    }

    if slots.env.is_empty()
        && slots.files.is_empty()
        && slots.libsecret.is_empty()
        && !resource_known_in_catalog
    {
        anyhow::bail!("unknown resource '{resource}'");
    }

    slots.env.sort_by(|a, b| a.entry.cmp(&b.entry));
    slots.files.sort_by(|a, b| a.entry.cmp(&b.entry));
    slots.libsecret.sort_by(|a, b| a.entry.cmp(&b.entry));

    Ok(underwrap_protocol::service::AdminUiResourceGetResponse {
        resource: resource.to_string(),
        slots,
    })
}

fn resource_filename(secret_id: &str) -> &str {
    secret_id.rsplit('/').next().unwrap_or("download")
}

fn json_response_no_store<T: Serialize>(status: StatusCode, body: &T) -> Response<Full<Bytes>> {
    let mut response = json_response(status, body);
    response.headers_mut().insert(
        hyper::header::CACHE_CONTROL,
        hyper::header::HeaderValue::from_static("no-store"),
    );
    response
}

fn admin_ui_resource_entry_summary(
    secret_id: &str,
    material: &[u8],
) -> Result<underwrap_protocol::service::AdminUiResourceEntrySummary> {
    let Some((_, slot, entry)) = split_resource_id(secret_id) else {
        anyhow::bail!("invalid resource secret id '{secret_id}'")
    };
    Ok(underwrap_protocol::service::AdminUiResourceEntrySummary {
        entry: entry.to_string(),
        secret_id: secret_id.to_string(),
        size_bytes: material.len(),
        is_utf8: looks_like_utf8_text(material),
        sha256_hex: (slot == "files").then(|| {
            use sha2::{Digest, Sha256};
            hex::encode(Sha256::digest(material))
        }),
    })
}

fn load_policy_document_optional(store: &dyn StorageBackend) -> Result<Option<PolicyDocument>> {
    match store.read(POLICY_DOCUMENT_SECRET_ID)? {
        Some(material) => serde_json::from_slice(material.as_ref())
            .context("parsing backend policy document")
            .map(Some),
        None => Ok(None),
    }
}

fn authorize_admin_ui_mutation(
    store: &dyn StorageBackend,
    wrapper: &str,
    secret_id: &str,
    now_epoch: u64,
) -> Result<()> {
    let Some(policy) = load_policy_document_optional(store)? else {
        return Ok(());
    };
    match policy.evaluate_after_approval_at(
        wrapper,
        &underwrap_protocol::policy::credential_resource(secret_id),
        underwrap_protocol::policy::OP_STORE_WRITE,
        now_epoch,
    ) {
        PolicyDecision::Allow => Ok(()),
        PolicyDecision::Deny { reason } => anyhow::bail!(reason),
        PolicyDecision::NeedsApproval { .. } => {
            anyhow::bail!("admin UI mutation does not support per-call approval mode")
        }
    }
}

fn admin_ui_mutation_grant_id(wrapper: &str, secret_id: &str) -> String {
    format!("ui-mutation:{wrapper}:{secret_id}")
}

fn handle_admin_ui_mutation_set(
    state: &AdminRuntimeState,
    operator: &str,
    body: underwrap_protocol::service::AdminUiMutationSetRequest,
    now_epoch: u64,
) -> Response<Full<Bytes>> {
    let store = match admin_store(state, &body.store) {
        Ok(store) => store,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    };
    if body.secret_id == POLICY_DOCUMENT_SECRET_ID {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "policy document is reserved",
        );
    }
    if split_resource_id(&body.secret_id).is_none() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "secret_id must identify an env, files, or libsecret entry",
        );
    }

    let grant_id = admin_ui_mutation_grant_id(&body.wrapper, &body.secret_id);
    let Some(mut policy) = (match load_policy_document_optional(store.as_ref()) {
        Ok(policy) => policy,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    }) else {
        let active = matches!(
            body.mode,
            underwrap_protocol::service::AdminUiMutationSetMode::Temporary
        );
        return json_response_no_store(
            StatusCode::OK,
            &underwrap_protocol::service::AdminUiMutationSetResponse {
                active,
                expires_at_epoch: body.expires_at_epoch,
                mode: if active {
                    "temporary".to_string()
                } else {
                    "disabled".to_string()
                },
            },
        );
    };

    match body.mode {
        underwrap_protocol::service::AdminUiMutationSetMode::Disabled => {
            policy.grants.retain(|grant| grant.id != grant_id);
            if let Err(e) = store.write(
                POLICY_DOCUMENT_SECRET_ID,
                &serde_json::to_vec(&policy).expect("serializing policy document"),
            ) {
                return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string());
            }
            info!(audit = "mutation_disabled", operator = %operator, store = %body.store, wrapper = %body.wrapper, secret = %body.secret_id);
            if let Err(e) = append_audit_history_event(
                store.as_ref(),
                underwrap_protocol::service::AdminUiAuditEvent {
                    ts_epoch: now_epoch,
                    action: "mutation_disabled".to_string(),
                    operator: operator.to_string(),
                    wrapper: Some(body.wrapper.clone()),
                    resource: split_resource_id(&body.secret_id)
                        .map(|(resource, _, _)| resource.to_string()),
                    secret_id: Some(body.secret_id.clone()),
                    operation: Some(underwrap_protocol::policy::OP_STORE_WRITE.to_string()),
                    result: "success".to_string(),
                },
            ) {
                warn!(store = %body.store, err = %e, "persisting audit event failed");
            }
            json_response_no_store(
                StatusCode::OK,
                &underwrap_protocol::service::AdminUiMutationSetResponse {
                    active: false,
                    expires_at_epoch: None,
                    mode: "disabled".to_string(),
                },
            )
        }
        underwrap_protocol::service::AdminUiMutationSetMode::Temporary => {
            let expires_at_epoch = body.expires_at_epoch.unwrap_or(now_epoch + 30 * 60);
            if expires_at_epoch <= now_epoch {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "expires_at_epoch must be in the future",
                );
            }
            let grant = underwrap_protocol::policy::PolicyGrant {
                id: grant_id,
                wrapper: body.wrapper.clone(),
                resource: underwrap_protocol::policy::credential_resource(&body.secret_id),
                operation: underwrap_protocol::policy::OP_STORE_WRITE.to_string(),
                effect: underwrap_protocol::policy::PolicyEffect::Allow,
                starts_at_epoch: Some(now_epoch),
                expires_at_epoch: Some(expires_at_epoch),
                reason: Some("admin-ui mutation window".to_string()),
            };
            if let Some(existing) = policy.grants.iter_mut().find(|entry| entry.id == grant.id) {
                *existing = grant;
            } else {
                policy.grants.push(grant);
            }
            policy.grants.sort_by(|a, b| a.id.cmp(&b.id));
            if let Err(e) = store.write(
                POLICY_DOCUMENT_SECRET_ID,
                &serde_json::to_vec(&policy).expect("serializing policy document"),
            ) {
                return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string());
            }
            info!(audit = "mutation_enabled", operator = %operator, store = %body.store, wrapper = %body.wrapper, secret = %body.secret_id, expires_at_epoch);
            if let Err(e) = append_audit_history_event(
                store.as_ref(),
                underwrap_protocol::service::AdminUiAuditEvent {
                    ts_epoch: now_epoch,
                    action: "mutation_enabled".to_string(),
                    operator: operator.to_string(),
                    wrapper: Some(body.wrapper.clone()),
                    resource: split_resource_id(&body.secret_id)
                        .map(|(resource, _, _)| resource.to_string()),
                    secret_id: Some(body.secret_id.clone()),
                    operation: Some(underwrap_protocol::policy::OP_STORE_WRITE.to_string()),
                    result: "success".to_string(),
                },
            ) {
                warn!(store = %body.store, err = %e, "persisting audit event failed");
            }
            json_response_no_store(
                StatusCode::OK,
                &underwrap_protocol::service::AdminUiMutationSetResponse {
                    active: true,
                    expires_at_epoch: Some(expires_at_epoch),
                    mode: "temporary".to_string(),
                },
            )
        }
    }
}

fn handle_admin_ui_resource_write_text(
    state: &AdminRuntimeState,
    operator: &str,
    body: underwrap_protocol::service::AdminUiResourceWriteTextRequest,
    now_epoch: u64,
) -> Response<Full<Bytes>> {
    if body.secret_id == POLICY_DOCUMENT_SECRET_ID {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "policy document is reserved",
        );
    }
    let Some((_, slot, _)) = split_resource_id(&body.secret_id) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "secret_id must identify an env or libsecret entry",
        );
    };
    if slot == "files" {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "file-backed entries must use the upload-file endpoint",
        );
    }
    let store = match admin_store(state, &body.store) {
        Ok(store) => store,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    };
    if let Err(e) =
        authorize_admin_ui_mutation(store.as_ref(), &body.wrapper, &body.secret_id, now_epoch)
    {
        return error_response(StatusCode::FORBIDDEN, "mutation_denied", &e.to_string());
    }
    let material = body.material_utf8.into_bytes();
    if material.len() > ADMIN_UI_MAX_FILE_BYTES {
        return error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "secret exceeds the 1 MiB admin UI upload limit",
        );
    }
    if let Err(e) = store.write(&body.secret_id, &material) {
        return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string());
    }
    info!(audit = "secret_stored", operator = %operator, store = %body.store, wrapper = %body.wrapper, secret = %body.secret_id);
    if let Err(e) = append_audit_history_event(
        store.as_ref(),
        underwrap_protocol::service::AdminUiAuditEvent {
            ts_epoch: now_epoch,
            action: "secret_stored".to_string(),
            operator: operator.to_string(),
            wrapper: Some(body.wrapper.clone()),
            resource: split_resource_id(&body.secret_id)
                .map(|(resource, _, _)| resource.to_string()),
            secret_id: Some(body.secret_id.clone()),
            operation: None,
            result: "success".to_string(),
        },
    ) {
        warn!(store = %body.store, err = %e, "persisting audit event failed");
    }
    match admin_ui_resource_entry_summary(&body.secret_id, &material) {
        Ok(entry) => json_response_no_store(
            StatusCode::OK,
            &underwrap_protocol::service::AdminUiResourceWriteResponse { entry },
        ),
        Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    }
}

fn handle_admin_ui_resource_upload_file(
    state: &AdminRuntimeState,
    operator: &str,
    body: underwrap_protocol::service::AdminUiResourceUploadFileRequest,
    now_epoch: u64,
) -> Response<Full<Bytes>> {
    if body.secret_id == POLICY_DOCUMENT_SECRET_ID {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "policy document is reserved",
        );
    }
    let Some((_, slot, _)) = split_resource_id(&body.secret_id) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "secret_id must identify a files entry",
        );
    };
    if slot != "files" {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "upload-file only accepts files slot entries",
        );
    }
    let material = match base64::engine::general_purpose::STANDARD.decode(&body.material_b64) {
        Ok(material) => material,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("invalid base64 payload: {e}"),
            );
        }
    };
    if material.len() > ADMIN_UI_MAX_FILE_BYTES {
        return error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "secret exceeds the 1 MiB admin UI upload limit",
        );
    }
    let store = match admin_store(state, &body.store) {
        Ok(store) => store,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    };
    if let Err(e) =
        authorize_admin_ui_mutation(store.as_ref(), &body.wrapper, &body.secret_id, now_epoch)
    {
        return error_response(StatusCode::FORBIDDEN, "mutation_denied", &e.to_string());
    }
    if let Err(e) = store.write(&body.secret_id, &material) {
        return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string());
    }
    info!(audit = "secret_stored", operator = %operator, store = %body.store, wrapper = %body.wrapper, secret = %body.secret_id);
    if let Err(e) = append_audit_history_event(
        store.as_ref(),
        underwrap_protocol::service::AdminUiAuditEvent {
            ts_epoch: now_epoch,
            action: "secret_stored".to_string(),
            operator: operator.to_string(),
            wrapper: Some(body.wrapper.clone()),
            resource: split_resource_id(&body.secret_id)
                .map(|(resource, _, _)| resource.to_string()),
            secret_id: Some(body.secret_id.clone()),
            operation: None,
            result: "success".to_string(),
        },
    ) {
        warn!(store = %body.store, err = %e, "persisting audit event failed");
    }
    match admin_ui_resource_entry_summary(&body.secret_id, &material) {
        Ok(entry) => json_response_no_store(
            StatusCode::OK,
            &underwrap_protocol::service::AdminUiResourceWriteResponse { entry },
        ),
        Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    }
}

fn handle_admin_ui_resource_delete_entry(
    state: &AdminRuntimeState,
    operator: &str,
    body: underwrap_protocol::service::AdminUiResourceDeleteEntryRequest,
    now_epoch: u64,
) -> Response<Full<Bytes>> {
    if body.secret_id == POLICY_DOCUMENT_SECRET_ID {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "policy document is reserved",
        );
    }
    if split_resource_id(&body.secret_id).is_none() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "secret_id must identify an env, files, or libsecret entry",
        );
    }
    let store = match admin_store(state, &body.store) {
        Ok(store) => store,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    };
    if let Err(e) =
        authorize_admin_ui_mutation(store.as_ref(), &body.wrapper, &body.secret_id, now_epoch)
    {
        return error_response(StatusCode::FORBIDDEN, "mutation_denied", &e.to_string());
    }
    match store.delete(&body.secret_id) {
        Ok(existed) => {
            if existed {
                info!(audit = "secret_deleted", operator = %operator, store = %body.store, wrapper = %body.wrapper, secret = %body.secret_id);
                if let Err(e) = append_audit_history_event(
                    store.as_ref(),
                    underwrap_protocol::service::AdminUiAuditEvent {
                        ts_epoch: now_epoch,
                        action: "secret_deleted".to_string(),
                        operator: operator.to_string(),
                        wrapper: Some(body.wrapper.clone()),
                        resource: split_resource_id(&body.secret_id)
                            .map(|(resource, _, _)| resource.to_string()),
                        secret_id: Some(body.secret_id.clone()),
                        operation: None,
                        result: "success".to_string(),
                    },
                ) {
                    warn!(store = %body.store, err = %e, "persisting audit event failed");
                }
            }
            json_response_no_store(
                StatusCode::OK,
                &underwrap_protocol::service::AdminUiResourceDeleteEntryResponse { existed },
            )
        }
        Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    }
}

fn handle_admin_ui_resource_reveal(
    state: &AdminRuntimeState,
    operator: &str,
    body: underwrap_protocol::service::AdminUiResourceRevealRequest,
) -> Response<Full<Bytes>> {
    let store = match admin_store(state, &body.store) {
        Ok(store) => store,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    };
    let material = match store.read(&body.secret_id) {
        Ok(Some(material)) => material,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "not_found", "unknown secret_id"),
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    };
    let material_ref = material.as_ref();
    if material_ref.len() > ADMIN_UI_MAX_FILE_BYTES {
        return error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "secret exceeds the 1 MiB admin UI reveal/download limit",
        );
    }

    info!(audit = "secret_revealed", operator = %operator, store = %body.store, secret = %body.secret_id);
    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    if let Err(e) = append_audit_history_event(
        store.as_ref(),
        underwrap_protocol::service::AdminUiAuditEvent {
            ts_epoch: now_epoch,
            action: "secret_revealed".to_string(),
            operator: operator.to_string(),
            wrapper: None,
            resource: split_resource_id(&body.secret_id)
                .map(|(resource, _, _)| resource.to_string()),
            secret_id: Some(body.secret_id.clone()),
            operation: None,
            result: "success".to_string(),
        },
    ) {
        warn!(store = %body.store, err = %e, "persisting audit event failed");
    }

    if looks_like_utf8_text(material_ref) {
        let text = std::str::from_utf8(material_ref).expect("text classification already verified");
        json_response_no_store(
            StatusCode::OK,
            &underwrap_protocol::service::AdminUiResourceRevealResponse::Utf8 {
                text: text.to_string(),
                size_bytes: material_ref.len(),
                copy_allowed: true,
            },
        )
    } else {
        json_response_no_store(
            StatusCode::OK,
            &underwrap_protocol::service::AdminUiResourceRevealResponse::Binary {
                base64: base64::engine::general_purpose::STANDARD.encode(material_ref),
                size_bytes: material_ref.len(),
                copy_allowed: false,
                download_recommended: true,
            },
        )
    }
}

fn handle_admin_ui_resource_download(
    state: &AdminRuntimeState,
    operator: &str,
    body: underwrap_protocol::service::AdminUiResourceDownloadRequest,
) -> Response<Full<Bytes>> {
    let store = match admin_store(state, &body.store) {
        Ok(store) => store,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    };
    let material = match store.read(&body.secret_id) {
        Ok(Some(material)) => material,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "not_found", "unknown secret_id"),
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    };
    let material_ref = material.as_ref();
    if material_ref.len() > ADMIN_UI_MAX_FILE_BYTES {
        return error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "secret exceeds the 1 MiB admin UI reveal/download limit",
        );
    }

    info!(audit = "secret_downloaded", operator = %operator, store = %body.store, secret = %body.secret_id);
    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    if let Err(e) = append_audit_history_event(
        store.as_ref(),
        underwrap_protocol::service::AdminUiAuditEvent {
            ts_epoch: now_epoch,
            action: "secret_downloaded".to_string(),
            operator: operator.to_string(),
            wrapper: None,
            resource: split_resource_id(&body.secret_id)
                .map(|(resource, _, _)| resource.to_string()),
            secret_id: Some(body.secret_id.clone()),
            operation: None,
            result: "success".to_string(),
        },
    ) {
        warn!(store = %body.store, err = %e, "persisting audit event failed");
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(hyper::header::CONTENT_TYPE, "application/octet-stream")
        .header(
            hyper::header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{}\"",
                resource_filename(&body.secret_id)
            ),
        )
        .header(hyper::header::CACHE_CONTROL, "no-store")
        .body(Full::new(Bytes::copy_from_slice(material_ref)))
        .expect("building admin UI download response")
}

fn admin_ui_mutation_get(
    store: &dyn StorageBackend,
    resource: &str,
    now_epoch: u64,
) -> Result<underwrap_protocol::service::AdminUiMutationGetResponse> {
    let secret_ids = store.list("")?;
    let catalog = load_catalog_document(store)?;
    let policy = load_policy_document(store)?;

    let resource_secret_ids: Vec<String> = secret_ids
        .into_iter()
        .filter(|secret_id| secret_id != POLICY_DOCUMENT_SECRET_ID && secret_id != CATALOG_DOCUMENT_SECRET_ID)
        .filter(|secret_id| matches!(split_resource_id(secret_id), Some((secret_resource, _, _)) if secret_resource == resource))
        .collect();

    let wrappers: BTreeSet<String> = catalog
        .iter()
        .filter(|entry| entry.resource == resource)
        .map(|entry| entry.wrapper.clone())
        .collect();

    let active_targets = active_mutation_targets(&policy, now_epoch);
    let mut states = Vec::new();
    for wrapper in wrappers {
        let matching_resources: Vec<String> = resource_secret_ids
            .iter()
            .map(|secret_id| underwrap_protocol::policy::credential_resource(secret_id))
            .collect();

        let permanent = policy.rules.iter().any(|rule| {
            rule.wrapper == wrapper
                && rule.operation == underwrap_protocol::policy::OP_STORE_WRITE
                && rule.effect == underwrap_protocol::policy::PolicyEffect::Allow
                && matching_resources
                    .iter()
                    .any(|resource_key| resource_key == &rule.resource)
        });

        let active_grants: Vec<&underwrap_protocol::policy::PolicyGrant> = policy
            .grants
            .iter()
            .filter(|grant| {
                grant.wrapper == wrapper
                    && grant.operation == underwrap_protocol::policy::OP_STORE_WRITE
                    && grant.effect == underwrap_protocol::policy::PolicyEffect::Allow
                    && matching_resources
                        .iter()
                        .any(|resource_key| resource_key == &grant.resource)
                    && grant.starts_at_epoch.is_none_or(|start| now_epoch >= start)
                    && grant.expires_at_epoch.is_none_or(|end| now_epoch < end)
            })
            .collect();

        let active = permanent
            || matching_resources.iter().any(|resource_key| {
                active_targets.contains(&(wrapper.clone(), resource_key.clone()))
            });

        let expires_at_epoch = if permanent {
            None
        } else {
            active_grants
                .iter()
                .filter_map(|grant| grant.expires_at_epoch)
                .min()
        };

        let mode = if permanent {
            "permanent"
        } else if active {
            "temporary"
        } else {
            "disabled"
        };

        states.push(underwrap_protocol::service::AdminUiMutationWrapperState {
            wrapper,
            active,
            expires_at_epoch,
            mode: mode.to_string(),
        });
    }
    states.sort_by(|a, b| a.wrapper.cmp(&b.wrapper));
    Ok(underwrap_protocol::service::AdminUiMutationGetResponse { wrappers: states })
}

fn handle_admin_ui_wrapper_set_mode(
    state: &AdminRuntimeState,
    operator: &str,
    body: underwrap_protocol::service::AdminUiWrapperSetModeRequest,
) -> Response<Full<Bytes>> {
    match admin_store(state, &body.store).and_then(|store| {
        let mut doc = load_policy_document(store.as_ref())?;
        if let Some(entry) = doc
            .wrappers
            .iter_mut()
            .find(|entry| entry.wrapper == body.wrapper)
        {
            entry.mode = body.mode.clone();
            entry.reason = body.reason.clone();
        } else {
            doc.wrappers
                .push(underwrap_protocol::policy::WrapperPolicyState {
                    wrapper: body.wrapper.clone(),
                    mode: body.mode.clone(),
                    reason: body.reason.clone(),
                });
        }
        doc.wrappers.sort_by(|a, b| a.wrapper.cmp(&b.wrapper));
        store.write(
            POLICY_DOCUMENT_SECRET_ID,
            serde_json::to_vec(&doc)?.as_slice(),
        )
    }) {
        Ok(()) => {
            info!(audit = "wrapper_mode_updated", operator = %operator, store = %body.store, wrapper = %body.wrapper, mode = ?body.mode);
            let now_epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0);
            if let Err(e) = append_audit_history_event(
                admin_store(state, &body.store)
                    .expect("store looked up earlier")
                    .as_ref(),
                underwrap_protocol::service::AdminUiAuditEvent {
                    ts_epoch: now_epoch,
                    action: "wrapper_mode_updated".to_string(),
                    operator: operator.to_string(),
                    wrapper: Some(body.wrapper.clone()),
                    resource: None,
                    secret_id: None,
                    operation: None,
                    result: "success".to_string(),
                },
            ) {
                warn!(store = %body.store, err = %e, "persisting audit event failed");
            }
            json_response_no_store(
                StatusCode::OK,
                &underwrap_protocol::service::AdminUiWrapperSetModeResponse {
                    wrapper: body.wrapper,
                    mode: body.mode,
                    reason: body.reason,
                },
            )
        }
        Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    }
}

fn admin_ui_matrix_rule_id(wrapper: &str, resource: &str, operation: &str) -> String {
    format!("matrix:{wrapper}:{resource}:{operation}")
}

fn admin_ui_matrix_rule_resource(resource: &str, operation: &str) -> Result<String> {
    match operation {
        underwrap_protocol::policy::OP_ENV_READ => Ok(format!("env:{resource}")),
        underwrap_protocol::policy::OP_FILES_READ | underwrap_protocol::policy::OP_FILES_WRITE => {
            Ok(format!("files:{resource}"))
        }
        underwrap_protocol::policy::OP_LIBSECRET_READ
        | underwrap_protocol::policy::OP_LIBSECRET_WRITE => Ok(format!("libsecret:{resource}")),
        other => anyhow::bail!("unknown matrix operation '{other}'"),
    }
}

fn handle_admin_ui_matrix_set_cell(
    state: &AdminRuntimeState,
    operator: &str,
    body: underwrap_protocol::service::AdminUiMatrixSetCellRequest,
) -> Response<Full<Bytes>> {
    let resource_key = match admin_ui_matrix_rule_resource(&body.resource, &body.operation) {
        Ok(resource_key) => resource_key,
        Err(e) => {
            return error_response(StatusCode::BAD_REQUEST, "invalid_request", &e.to_string());
        }
    };
    let rule_id = admin_ui_matrix_rule_id(&body.wrapper, &body.resource, &body.operation);

    match admin_store(state, &body.store).and_then(|store| {
        let mut doc = load_policy_document(store.as_ref())?;
        if body.enabled {
            let rule = underwrap_protocol::policy::PolicyRule {
                id: rule_id.clone(),
                wrapper: body.wrapper.clone(),
                resource: resource_key,
                operation: body.operation.clone(),
                effect: underwrap_protocol::policy::PolicyEffect::Allow,
            };
            if let Some(existing) = doc.rules.iter_mut().find(|existing| existing.id == rule.id) {
                *existing = rule;
            } else {
                doc.rules.push(rule);
            }
            doc.rules.sort_by(|a, b| a.id.cmp(&b.id));
        } else {
            doc.rules.retain(|rule| rule.id != rule_id);
        }
        store.write(
            POLICY_DOCUMENT_SECRET_ID,
            serde_json::to_vec(&doc)?.as_slice(),
        )
    }) {
        Ok(()) => {
            info!(audit = if body.enabled { "matrix_cell_enabled" } else { "matrix_cell_disabled" }, operator = %operator, store = %body.store, wrapper = %body.wrapper, resource = %body.resource, operation = %body.operation);
            let now_epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0);
            if let Err(e) = append_audit_history_event(
                admin_store(state, &body.store)
                    .expect("store looked up earlier")
                    .as_ref(),
                underwrap_protocol::service::AdminUiAuditEvent {
                    ts_epoch: now_epoch,
                    action: if body.enabled {
                        "matrix_cell_enabled".to_string()
                    } else {
                        "matrix_cell_disabled".to_string()
                    },
                    operator: operator.to_string(),
                    wrapper: Some(body.wrapper.clone()),
                    resource: Some(body.resource.clone()),
                    secret_id: None,
                    operation: Some(body.operation.clone()),
                    result: "success".to_string(),
                },
            ) {
                warn!(store = %body.store, err = %e, "persisting audit event failed");
            }
            json_response_no_store(
                StatusCode::OK,
                &underwrap_protocol::service::AdminUiMatrixSetCellResponse {
                    enabled: body.enabled,
                },
            )
        }
        Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    }
}

fn admin_ui_wrappers_query(
    store: &dyn StorageBackend,
    request: &underwrap_protocol::service::AdminUiWrappersQueryRequest,
) -> Result<underwrap_protocol::service::AdminUiWrappersQueryResponse> {
    let catalog = load_catalog_document(store)?;
    let policy = load_policy_document(store)?;

    let mut wrappers: BTreeSet<String> =
        catalog.iter().map(|entry| entry.wrapper.clone()).collect();
    wrappers.extend(policy.wrappers.iter().map(|entry| entry.wrapper.clone()));

    let query = request.query.to_ascii_lowercase();
    let mut items = Vec::new();
    for wrapper in wrappers {
        let mode = policy.wrapper_mode(&wrapper);
        let reason = policy
            .wrappers
            .iter()
            .find(|entry| entry.wrapper == wrapper)
            .and_then(|entry| entry.reason.clone());
        let resource_count = catalog
            .iter()
            .filter(|entry| entry.wrapper == wrapper)
            .map(|entry| entry.resource.clone())
            .collect::<BTreeSet<_>>()
            .len();

        let mode_matches = match request.mode.as_deref() {
            None | Some("any") => true,
            Some("locked") => matches!(mode, WrapperApprovalMode::Locked),
            Some("unlocked") => matches!(mode, WrapperApprovalMode::Unlocked { until_epoch: None }),
            Some("unlocked_until") => matches!(
                mode,
                WrapperApprovalMode::Unlocked {
                    until_epoch: Some(_)
                }
            ),
            Some("per_call_approval") => {
                matches!(mode, WrapperApprovalMode::PerCallApproval { .. })
            }
            Some(other) => anyhow::bail!("unknown mode filter '{other}'"),
        };
        let query_matches = query.is_empty() || wrapper.to_ascii_lowercase().contains(&query);
        if mode_matches && query_matches {
            items.push(underwrap_protocol::service::AdminUiWrapperListItem {
                wrapper,
                mode,
                reason,
                resource_count,
            });
        }
    }
    items.sort_by(|a, b| a.wrapper.cmp(&b.wrapper));
    Ok(underwrap_protocol::service::AdminUiWrappersQueryResponse { items })
}

fn admin_ui_matrix_query(
    store: &dyn StorageBackend,
    request: &underwrap_protocol::service::AdminUiMatrixQueryRequest,
    now_epoch: u64,
) -> Result<underwrap_protocol::service::AdminUiMatrixQueryResponse> {
    let catalog = load_catalog_document(store)?;
    let policy = load_policy_document(store)?;
    let operations = vec![
        underwrap_protocol::policy::OP_ENV_READ.to_string(),
        underwrap_protocol::policy::OP_FILES_READ.to_string(),
        underwrap_protocol::policy::OP_FILES_WRITE.to_string(),
        underwrap_protocol::policy::OP_LIBSECRET_READ.to_string(),
        underwrap_protocol::policy::OP_LIBSECRET_WRITE.to_string(),
    ];

    let mut rows = Vec::new();
    for entry in catalog.into_iter().filter(|entry| {
        request
            .resource
            .as_deref()
            .is_none_or(|resource| entry.resource == resource)
            && request
                .host
                .as_deref()
                .is_none_or(|host| entry.host == host)
            && request
                .user
                .as_deref()
                .is_none_or(|user| entry.user == user)
            && request
                .wrapper
                .as_deref()
                .is_none_or(|wrapper| entry.wrapper == wrapper)
    }) {
        let mut cells = std::collections::BTreeMap::new();
        for operation in &operations {
            let resource_key = match operation.as_str() {
                underwrap_protocol::policy::OP_ENV_READ => format!("env:{}", entry.resource),
                underwrap_protocol::policy::OP_FILES_READ
                | underwrap_protocol::policy::OP_FILES_WRITE => format!("files:{}", entry.resource),
                underwrap_protocol::policy::OP_LIBSECRET_READ
                | underwrap_protocol::policy::OP_LIBSECRET_WRITE => {
                    format!("libsecret:{}", entry.resource)
                }
                _ => continue,
            };
            let allowed = matches!(
                policy.evaluate_after_approval_at(
                    &entry.wrapper,
                    &resource_key,
                    operation,
                    now_epoch
                ),
                PolicyDecision::Allow
            );
            cells.insert(operation.clone(), allowed);
        }
        rows.push(underwrap_protocol::service::AdminUiMatrixRow {
            host: entry.host,
            user: entry.user,
            wrapper: entry.wrapper,
            cells,
        });
    }
    rows.sort_by(|a, b| {
        a.host
            .cmp(&b.host)
            .then(a.user.cmp(&b.user))
            .then(a.wrapper.cmp(&b.wrapper))
    });

    Ok(underwrap_protocol::service::AdminUiMatrixQueryResponse { operations, rows })
}

fn admin_ui_resources_query(
    store: &dyn StorageBackend,
    request: &underwrap_protocol::service::AdminUiResourcesQueryRequest,
    now_epoch: u64,
) -> Result<underwrap_protocol::service::AdminUiResourcesQueryResponse> {
    let secret_ids = store.list("")?;
    let catalog = load_catalog_document(store)?;
    let policy = load_policy_document(store)?;
    let mutation_targets = active_mutation_targets(&policy, now_epoch);

    let mut resources = BTreeSet::new();
    let mut resource_secret_ids: HashMap<String, Vec<String>> = HashMap::new();
    for secret_id in secret_ids {
        if secret_id == POLICY_DOCUMENT_SECRET_ID || secret_id == CATALOG_DOCUMENT_SECRET_ID {
            continue;
        }
        if let Some((resource, _, _)) = split_resource_id(&secret_id) {
            resources.insert(resource.to_string());
            resource_secret_ids
                .entry(resource.to_string())
                .or_default()
                .push(secret_id);
        }
    }
    for entry in &catalog {
        resources.insert(entry.resource.clone());
    }

    let mut items = Vec::new();
    for resource in resources {
        let ids = resource_secret_ids
            .get(&resource)
            .cloned()
            .unwrap_or_default();
        let slots = underwrap_protocol::service::AdminUiResourceSlots {
            env: ids
                .iter()
                .any(|id| matches!(split_resource_id(id), Some((_, "env", _)))),
            files: ids
                .iter()
                .any(|id| matches!(split_resource_id(id), Some((_, "files", _)))),
            libsecret: ids
                .iter()
                .any(|id| matches!(split_resource_id(id), Some((_, "libsecret", _)))),
        };
        let mut catalog_wrappers: Vec<String> = catalog
            .iter()
            .filter(|entry| entry.resource == resource)
            .map(|entry| entry.wrapper.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        catalog_wrappers.sort();

        let mutation_active = ids.iter().any(|secret_id| {
            let resource_key = underwrap_protocol::policy::credential_resource(secret_id);
            catalog_wrappers
                .iter()
                .any(|wrapper| mutation_targets.contains(&(wrapper.clone(), resource_key.clone())))
        });

        let item = underwrap_protocol::service::AdminUiResourceListItem {
            resource,
            slots,
            catalog_wrappers,
            mutation: underwrap_protocol::service::AdminUiResourceMutationSummary {
                active: mutation_active,
            },
        };
        if resource_query_matches(&item, &request.query)
            && slot_filter_matches(&item, request.slot.as_deref())?
            && mutation_filter_matches(&item, request.mutation_state.as_deref())?
        {
            items.push(item);
        }
    }

    let total = items.len();
    let page = request.page.max(1);
    let page_size = request.page_size.clamp(1, 200);
    let start = page_size.saturating_mul(page.saturating_sub(1));
    let items = items.into_iter().skip(start).take(page_size).collect();

    Ok(underwrap_protocol::service::AdminUiResourcesQueryResponse { items, total })
}

fn store_catalog_document(
    store: &dyn StorageBackend,
    entries: &[StoredCatalogEntry],
) -> Result<()> {
    store.write(
        CATALOG_DOCUMENT_SECRET_ID,
        serde_json::to_vec(entries)?.as_slice(),
    )
}

fn replace_catalog_entries_for_principal(
    store: &dyn StorageBackend,
    principal: &str,
    entries: &[underwrap_protocol::service::CatalogEntry],
) -> Result<()> {
    let mut existing = load_catalog_document(store)?;
    existing.retain(|entry| entry.principal != principal);
    for entry in entries {
        existing.push(StoredCatalogEntry {
            principal: principal.to_string(),
            host: entry.host.clone(),
            user: entry.user.clone(),
            instance: entry.instance.clone(),
            wrapper: entry.wrapper.clone(),
            resource: entry.resource.clone(),
        });
    }
    existing.sort_by(|a, b| {
        a.resource
            .cmp(&b.resource)
            .then(a.host.cmp(&b.host))
            .then(a.user.cmp(&b.user))
            .then(a.instance.cmp(&b.instance))
            .then(a.wrapper.cmp(&b.wrapper))
            .then(a.principal.cmp(&b.principal))
    });
    existing.dedup();
    store_catalog_document(store, &existing)
}

fn admin_store<'a>(
    state: &'a AdminRuntimeState,
    store: &str,
) -> Result<&'a Arc<dyn StorageBackend>> {
    state
        .stores
        .get(store)
        .with_context(|| format!("unknown store '{store}'"))
}

/// Read each admin token file, trim whitespace, and return the loaded map.
fn load_admin_tokens(
    configs: &HashMap<String, AdminTokenConfig>,
) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    for (name, entry) in configs {
        let raw = std::fs::read_to_string(&entry.token_file).with_context(|| {
            format!(
                "reading admin token for operator '{name}' at {}",
                entry.token_file.display()
            )
        })?;
        let token = raw.trim().to_string();
        anyhow::ensure!(
            !token.is_empty(),
            "admin token file for operator '{name}' at {} is empty",
            entry.token_file.display()
        );
        out.insert(name.clone(), token);
    }
    Ok(out)
}

/// Extract the bearer token (after "Bearer ") from an Authorization header,
/// matching it in constant time against any configured operator token.
/// Returns the operator name on success.
fn authenticate_admin(
    state: &AdminRuntimeState,
    headers: &hyper::HeaderMap,
) -> std::result::Result<String, &'static str> {
    use sha2::{Digest, Sha256};
    use subtle::ConstantTimeEq;

    let header = headers
        .get(hyper::header::AUTHORIZATION)
        .ok_or("missing Authorization header")?
        .to_str()
        .map_err(|_| "Authorization header is not valid UTF-8")?;
    let token = header
        .strip_prefix("Bearer ")
        .ok_or("Authorization header must use the Bearer scheme")?
        .trim();
    if token.is_empty() {
        return Err("empty bearer token");
    }

    let token_hash = Sha256::digest(token.as_bytes());
    let mut matched: Option<String> = None;
    for (name, candidate) in &state.tokens {
        let candidate_hash = Sha256::digest(candidate.as_bytes());
        let is_match: bool = token_hash.ct_eq(&candidate_hash).into();
        if is_match && matched.is_none() {
            matched = Some(name.clone());
        }
    }
    matched.ok_or("no configured operator token matched")
}

/// How often the expiry sweeper checks for expired `Unlocked { until }` windows.
const EXPIRY_SWEEP_INTERVAL: Duration = Duration::from_secs(30);

async fn expiry_sweeper_loop(stores: HashMap<String, Arc<dyn StorageBackend>>) -> Result<()> {
    let mut ticker = interval(EXPIRY_SWEEP_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        for (name, store) in &stores {
            let now_epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            match sweep_expired_unlocks(store.as_ref(), now_epoch) {
                Ok(0) => {}
                Ok(n) => info!(store = %name, expired = n, "swept expired wrapper unlocks"),
                Err(e) => warn!(store = %name, err = %e, "expiry sweep failed"),
            }
        }
    }
}

/// Walk the policy document in `store`, flip every wrapper whose
/// `Unlocked { until_epoch }` has elapsed to `Locked`, and persist.
/// Returns the count of wrappers transitioned.
fn sweep_expired_unlocks(store: &dyn StorageBackend, now_epoch: u64) -> Result<usize> {
    let Some(material) = store.read(POLICY_DOCUMENT_SECRET_ID)? else {
        return Ok(0);
    };
    let mut doc: PolicyDocument = serde_json::from_slice(material.as_ref())
        .context("parsing policy document during expiry sweep")?;

    let mut changed = 0usize;
    for entry in doc.wrappers.iter_mut() {
        if let WrapperApprovalMode::Unlocked {
            until_epoch: Some(until),
        } = entry.mode
            && now_epoch > until
        {
            entry.mode = WrapperApprovalMode::Locked;
            entry.reason = Some(format!(
                "auto-locked: unlocked window expired at epoch {until}"
            ));
            changed += 1;
        }
    }

    if changed > 0 {
        store.write(
            POLICY_DOCUMENT_SECRET_ID,
            serde_json::to_vec(&doc)?.as_slice(),
        )?;
    }
    Ok(changed)
}

async fn handle_admin_request(
    request: Request<Incoming>,
    state: Arc<AdminRuntimeState>,
) -> std::result::Result<Response<Full<Bytes>>, std::convert::Infallible> {
    if let Err(reason) = check_admin_origin(
        request.method(),
        request.headers(),
        state.allowed_origins.as_deref(),
    ) {
        warn!(reason = reason, "rejected admin request (origin check)");
        return Ok(error_response(StatusCode::FORBIDDEN, "bad_origin", reason));
    }
    let operator = match authenticate_admin(&state, request.headers()) {
        Ok(op) => op,
        Err(reason) => {
            warn!(reason = reason, "rejected admin request");
            return Ok(error_response(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                reason,
            ));
        }
    };
    info!(
        operator = %operator,
        method = %request.method(),
        path = request.uri().path(),
        "admin request"
    );

    let response = match (request.method(), request.uri().path()) {
        (&Method::GET, "/v1/admin/stores") => {
            let mut stores: Vec<String> = state.stores.keys().cloned().collect();
            stores.sort();
            json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminStoresResponse { stores },
            )
        }
        (&Method::GET, "/v1/admin/ui/overview") => {
            let now_epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0);
            match state
                .stores
                .iter()
                .map(|(name, store)| admin_ui_overview_for_store(name, store.as_ref(), now_epoch))
                .collect::<Result<Vec<_>>>()
            {
                Ok(mut stores) => {
                    stores.sort_by(|a, b| a.store.cmp(&b.store));
                    json_response(
                        StatusCode::OK,
                        &underwrap_protocol::service::AdminUiOverviewResponse { stores },
                    )
                }
                Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
            }
        }
        (&Method::POST, "/v1/admin/ui/resources/query") => match parse_json_body::<
            underwrap_protocol::service::AdminUiResourcesQueryRequest,
        >(request)
        .await
        {
            Ok(body) => {
                let now_epoch = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0);
                match admin_store(&state, &body.store)
                    .and_then(|store| admin_ui_resources_query(store.as_ref(), &body, now_epoch))
                {
                    Ok(response) => json_response(StatusCode::OK, &response),
                    Err(e) => {
                        error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                    }
                }
            }
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/resource/get") => match parse_json_body::<
            underwrap_protocol::service::AdminUiResourceGetRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store(&state, &body.store)
                .and_then(|store| admin_ui_resource_detail(store.as_ref(), &body.resource))
            {
                Ok(response) => json_response(StatusCode::OK, &response),
                Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/resource/reveal") => match parse_json_body::<
            underwrap_protocol::service::AdminUiResourceRevealRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_ui_resource_reveal(&state, &operator, body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/resource/download") => match parse_json_body::<
            underwrap_protocol::service::AdminUiResourceDownloadRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_ui_resource_download(&state, &operator, body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/resource/write-text") => match parse_json_body::<
            underwrap_protocol::service::AdminUiResourceWriteTextRequest,
        >(request)
        .await
        {
            Ok(body) => {
                let now_epoch = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0);
                handle_admin_ui_resource_write_text(&state, &operator, body, now_epoch)
            }
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/resource/upload-file") => match parse_json_body::<
            underwrap_protocol::service::AdminUiResourceUploadFileRequest,
        >(request)
        .await
        {
            Ok(body) => {
                let now_epoch = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0);
                handle_admin_ui_resource_upload_file(&state, &operator, body, now_epoch)
            }
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/resource/delete-entry") => match parse_json_body::<
            underwrap_protocol::service::AdminUiResourceDeleteEntryRequest,
        >(request)
        .await
        {
            Ok(body) => {
                let now_epoch = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0);
                handle_admin_ui_resource_delete_entry(&state, &operator, body, now_epoch)
            }
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/mutation/get") => {
            match parse_json_body::<underwrap_protocol::service::AdminUiMutationGetRequest>(request)
                .await
            {
                Ok(body) => {
                    let now_epoch = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|duration| duration.as_secs())
                        .unwrap_or(0);
                    match admin_store(&state, &body.store).and_then(|store| {
                        admin_ui_mutation_get(store.as_ref(), &body.resource, now_epoch)
                    }) {
                        Ok(response) => json_response(StatusCode::OK, &response),
                        Err(e) => {
                            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                        }
                    }
                }
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/ui/mutation/set") => {
            match parse_json_body::<underwrap_protocol::service::AdminUiMutationSetRequest>(request)
                .await
            {
                Ok(body) => {
                    let now_epoch = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|duration| duration.as_secs())
                        .unwrap_or(0);
                    handle_admin_ui_mutation_set(&state, &operator, body, now_epoch)
                }
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/ui/wrappers/query") => match parse_json_body::<
            underwrap_protocol::service::AdminUiWrappersQueryRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store(&state, &body.store)
                .and_then(|store| admin_ui_wrappers_query(store.as_ref(), &body))
            {
                Ok(response) => json_response(StatusCode::OK, &response),
                Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/wrapper/set-mode") => match parse_json_body::<
            underwrap_protocol::service::AdminUiWrapperSetModeRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_ui_wrapper_set_mode(&state, &operator, body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/matrix/query") => {
            match parse_json_body::<underwrap_protocol::service::AdminUiMatrixQueryRequest>(request)
                .await
            {
                Ok(body) => {
                    let now_epoch = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|duration| duration.as_secs())
                        .unwrap_or(0);
                    match admin_store(&state, &body.store)
                        .and_then(|store| admin_ui_matrix_query(store.as_ref(), &body, now_epoch))
                    {
                        Ok(response) => json_response(StatusCode::OK, &response),
                        Err(e) => {
                            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                        }
                    }
                }
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/ui/matrix/set-cell") => match parse_json_body::<
            underwrap_protocol::service::AdminUiMatrixSetCellRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_ui_matrix_set_cell(&state, &operator, body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/audit/query") => match parse_json_body::<
            underwrap_protocol::service::AdminUiAuditQueryRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store(&state, &body.store)
                .and_then(|store| admin_ui_audit_query(store.as_ref(), &body))
            {
                Ok(response) => json_response_no_store(StatusCode::OK, &response),
                Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/secret/read") => match parse_json_body::<
            underwrap_protocol::service::AdminSecretReadRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store(&state, &body.store)
                .and_then(|store| store.read(&body.secret_id))
            {
                Ok(material) => json_response(
                    StatusCode::OK,
                    &underwrap_protocol::service::AdminSecretReadResponse {
                        material_b64: material.map(|material| {
                            base64::engine::general_purpose::STANDARD.encode(material.as_ref())
                        }),
                    },
                ),
                Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/secret/write") => {
            match parse_json_body::<underwrap_protocol::service::AdminSecretWriteRequest>(request)
                .await
            {
                Ok(body) => handle_admin_secret_write(&state, &operator, body),
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/secret/list") => match parse_json_body::<
            underwrap_protocol::service::AdminSecretListRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store(&state, &body.store)
                .and_then(|store| store.list(&body.prefix))
            {
                Ok(ids) => json_response(
                    StatusCode::OK,
                    &underwrap_protocol::service::AdminSecretListResponse { ids },
                ),
                Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/secret/delete") => match parse_json_body::<
            underwrap_protocol::service::AdminSecretDeleteRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store(&state, &body.store)
                .and_then(|store| store.delete(&body.secret_id))
            {
                Ok(existed) => json_response(
                    StatusCode::OK,
                    &underwrap_protocol::service::AdminSecretDeleteResponse { existed },
                ),
                Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/catalog/get") => match parse_json_body::<
            underwrap_protocol::service::AdminCatalogGetRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store(&state, &body.store)
                .and_then(|store| load_catalog_document(&**store))
            {
                Ok(entries) => json_response(
                    StatusCode::OK,
                    &underwrap_protocol::service::AdminCatalogGetResponse {
                        entries: entries
                            .into_iter()
                            .map(|entry| underwrap_protocol::service::CatalogEntry {
                                host: entry.host,
                                user: entry.user,
                                instance: entry.instance,
                                wrapper: entry.wrapper,
                                resource: entry.resource,
                            })
                            .collect(),
                    },
                ),
                Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/policy/get") => {
            match parse_json_body::<underwrap_protocol::service::AdminPolicyGetRequest>(request)
                .await
            {
                Ok(body) => {
                    let doc = match admin_store(&state, &body.store).and_then(|store| {
                        match store.read(POLICY_DOCUMENT_SECRET_ID)? {
                            Some(material) => Ok(serde_json::from_slice(material.as_ref())?),
                            None => Ok(underwrap_protocol::policy::PolicyDocument::default()),
                        }
                    }) {
                        Ok(doc) => doc,
                        Err(e) => {
                            return Ok(error_response(
                                StatusCode::BAD_REQUEST,
                                "admin_error",
                                &e.to_string(),
                            ));
                        }
                    };
                    json_response(
                        StatusCode::OK,
                        &underwrap_protocol::service::AdminPolicyGetResponse { document: doc },
                    )
                }
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/policy/put") => match parse_json_body::<
            underwrap_protocol::service::AdminPolicyPutRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store(&state, &body.store).and_then(|store| {
                store.write(
                    POLICY_DOCUMENT_SECRET_ID,
                    serde_json::to_vec(&body.document)?.as_slice(),
                )
            }) {
                Ok(()) => json_response(
                    StatusCode::OK,
                    &underwrap_protocol::service::AdminOkResponse { ok: true },
                ),
                Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/wrapper/set-mode") => match parse_json_body::<
            underwrap_protocol::service::AdminWrapperModeRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store(&state, &body.store).and_then(|store| {
                let mut doc: underwrap_protocol::policy::PolicyDocument =
                    match store.read(POLICY_DOCUMENT_SECRET_ID)? {
                        Some(material) => serde_json::from_slice(material.as_ref())?,
                        None => underwrap_protocol::policy::PolicyDocument::default(),
                    };
                if let Some(state) = doc
                    .wrappers
                    .iter_mut()
                    .find(|state| state.wrapper == body.wrapper)
                {
                    state.mode = body.mode.clone();
                    state.reason = body.reason.clone();
                } else {
                    doc.wrappers
                        .push(underwrap_protocol::policy::WrapperPolicyState {
                            wrapper: body.wrapper.clone(),
                            mode: body.mode.clone(),
                            reason: body.reason.clone(),
                        });
                }
                doc.wrappers.sort_by(|a, b| a.wrapper.cmp(&b.wrapper));
                store.write(
                    POLICY_DOCUMENT_SECRET_ID,
                    serde_json::to_vec(&doc)?.as_slice(),
                )
            }) {
                Ok(()) => json_response(
                    StatusCode::OK,
                    &underwrap_protocol::service::AdminOkResponse { ok: true },
                ),
                Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/rule/upsert") => match parse_json_body::<
            underwrap_protocol::service::AdminRuleUpsertRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store(&state, &body.store).and_then(|store| {
                let mut doc: underwrap_protocol::policy::PolicyDocument =
                    match store.read(POLICY_DOCUMENT_SECRET_ID)? {
                        Some(material) => serde_json::from_slice(material.as_ref())?,
                        None => underwrap_protocol::policy::PolicyDocument::default(),
                    };
                if let Some(rule) = doc.rules.iter_mut().find(|rule| rule.id == body.rule.id) {
                    *rule = body.rule.clone();
                } else {
                    doc.rules.push(body.rule.clone());
                }
                doc.rules.sort_by(|a, b| a.id.cmp(&b.id));
                store.write(
                    POLICY_DOCUMENT_SECRET_ID,
                    serde_json::to_vec(&doc)?.as_slice(),
                )
            }) {
                Ok(()) => json_response(
                    StatusCode::OK,
                    &underwrap_protocol::service::AdminOkResponse { ok: true },
                ),
                Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/rule/delete") => match parse_json_body::<
            underwrap_protocol::service::AdminRuleDeleteRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store(&state, &body.store).and_then(|store| {
                let mut doc: underwrap_protocol::policy::PolicyDocument =
                    match store.read(POLICY_DOCUMENT_SECRET_ID)? {
                        Some(material) => serde_json::from_slice(material.as_ref())?,
                        None => underwrap_protocol::policy::PolicyDocument::default(),
                    };
                doc.rules.retain(|rule| rule.id != body.id);
                store.write(
                    POLICY_DOCUMENT_SECRET_ID,
                    serde_json::to_vec(&doc)?.as_slice(),
                )
            }) {
                Ok(()) => json_response(
                    StatusCode::OK,
                    &underwrap_protocol::service::AdminOkResponse { ok: true },
                ),
                Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/grant/upsert") => match parse_json_body::<
            underwrap_protocol::service::AdminGrantUpsertRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store(&state, &body.store).and_then(|store| {
                let mut doc: underwrap_protocol::policy::PolicyDocument =
                    match store.read(POLICY_DOCUMENT_SECRET_ID)? {
                        Some(material) => serde_json::from_slice(material.as_ref())?,
                        None => underwrap_protocol::policy::PolicyDocument::default(),
                    };
                if let Some(grant) = doc
                    .grants
                    .iter_mut()
                    .find(|grant| grant.id == body.grant.id)
                {
                    *grant = body.grant.clone();
                } else {
                    doc.grants.push(body.grant.clone());
                }
                doc.grants.sort_by(|a, b| a.id.cmp(&b.id));
                store.write(
                    POLICY_DOCUMENT_SECRET_ID,
                    serde_json::to_vec(&doc)?.as_slice(),
                )
            }) {
                Ok(()) => json_response(
                    StatusCode::OK,
                    &underwrap_protocol::service::AdminOkResponse { ok: true },
                ),
                Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/grant/delete") => match parse_json_body::<
            underwrap_protocol::service::AdminGrantDeleteRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store(&state, &body.store).and_then(|store| {
                let mut doc: underwrap_protocol::policy::PolicyDocument =
                    match store.read(POLICY_DOCUMENT_SECRET_ID)? {
                        Some(material) => serde_json::from_slice(material.as_ref())?,
                        None => underwrap_protocol::policy::PolicyDocument::default(),
                    };
                doc.grants.retain(|grant| grant.id != body.id);
                store.write(
                    POLICY_DOCUMENT_SECRET_ID,
                    serde_json::to_vec(&doc)?.as_slice(),
                )
            }) {
                Ok(()) => json_response(
                    StatusCode::OK,
                    &underwrap_protocol::service::AdminOkResponse { ok: true },
                ),
                Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
            },
            Err(response) => response,
        },
        _ => error_response(StatusCode::NOT_FOUND, "not_found", "unknown admin endpoint"),
    };
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;
    use underwrap_protocol::message::SecretBytes;

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
    fn put_policy_document_rejects_invalid_json() {
        let stores = HashMap::from([(
            "work".to_string(),
            Arc::new(TestStore::new()) as Arc<dyn StorageBackend>,
        )]);

        let err = put_policy_document(&stores, "work", "not-json").unwrap_err();
        assert!(err.to_string().contains("parsing policy document json"));
    }

    fn write_policy(store: &TestStore, doc: &PolicyDocument) {
        store
            .write(POLICY_DOCUMENT_SECRET_ID, &serde_json::to_vec(doc).unwrap())
            .unwrap();
    }

    fn read_policy(store: &TestStore) -> PolicyDocument {
        let bytes = store.read(POLICY_DOCUMENT_SECRET_ID).unwrap().unwrap();
        serde_json::from_slice(bytes.as_ref()).unwrap()
    }

    #[test]
    fn sweep_expired_unlocks_is_a_no_op_when_nothing_expired() {
        let store = TestStore::new();
        let doc = PolicyDocument {
            wrappers: vec![underwrap_protocol::policy::WrapperPolicyState {
                wrapper: "gh".to_string(),
                mode: WrapperApprovalMode::Unlocked {
                    until_epoch: Some(1000),
                },
                reason: None,
            }],
            rules: Vec::new(),
            grants: Vec::new(),
        };
        write_policy(&store, &doc);

        assert_eq!(sweep_expired_unlocks(&store, 500).unwrap(), 0);
        assert_eq!(read_policy(&store), doc);
    }

    #[test]
    fn sweep_expired_unlocks_locks_expired_wrapper_and_records_reason() {
        let store = TestStore::new();
        let doc = PolicyDocument {
            wrappers: vec![
                underwrap_protocol::policy::WrapperPolicyState {
                    wrapper: "gh".to_string(),
                    mode: WrapperApprovalMode::Unlocked {
                        until_epoch: Some(100),
                    },
                    reason: None,
                },
                underwrap_protocol::policy::WrapperPolicyState {
                    wrapper: "jira".to_string(),
                    mode: WrapperApprovalMode::Unlocked { until_epoch: None },
                    reason: None,
                },
            ],
            rules: Vec::new(),
            grants: Vec::new(),
        };
        write_policy(&store, &doc);

        assert_eq!(sweep_expired_unlocks(&store, 200).unwrap(), 1);

        let after = read_policy(&store);
        assert_eq!(after.wrappers[0].mode, WrapperApprovalMode::Locked);
        assert!(
            after.wrappers[0]
                .reason
                .as_deref()
                .unwrap_or_default()
                .contains("auto-locked")
        );
        // Wrapper without expiry untouched.
        assert_eq!(
            after.wrappers[1].mode,
            WrapperApprovalMode::Unlocked { until_epoch: None }
        );
    }

    #[test]
    fn sweep_expired_unlocks_is_noop_when_no_policy_document_exists() {
        let store = TestStore::new();
        assert_eq!(sweep_expired_unlocks(&store, 1000).unwrap(), 0);
    }

    #[test]
    fn replace_catalog_entries_replaces_only_one_principal_slice() {
        let store = TestStore::new();
        replace_catalog_entries_for_principal(
            &store,
            "principal-a",
            &[
                underwrap_protocol::service::CatalogEntry {
                    host: "deepthought".to_string(),
                    user: "robert".to_string(),
                    instance: "robert".to_string(),
                    wrapper: "gh-authorized".to_string(),
                    resource: "gh/main".to_string(),
                },
                underwrap_protocol::service::CatalogEntry {
                    host: "deepthought".to_string(),
                    user: "robert".to_string(),
                    instance: "robert".to_string(),
                    wrapper: "gws-castlabs".to_string(),
                    resource: "gws/castlabs".to_string(),
                },
            ],
        )
        .unwrap();
        replace_catalog_entries_for_principal(
            &store,
            "principal-b",
            &[underwrap_protocol::service::CatalogEntry {
                host: "smallthought".to_string(),
                user: "robert".to_string(),
                instance: "robert".to_string(),
                wrapper: "gh-authorized".to_string(),
                resource: "gh/main".to_string(),
            }],
        )
        .unwrap();
        replace_catalog_entries_for_principal(
            &store,
            "principal-a",
            &[underwrap_protocol::service::CatalogEntry {
                host: "deepthought".to_string(),
                user: "robert".to_string(),
                instance: "robert".to_string(),
                wrapper: "confluence".to_string(),
                resource: "confluence/main".to_string(),
            }],
        )
        .unwrap();

        let entries = load_catalog_document(&store).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.principal == "principal-a"
            && e.instance == "robert"
            && e.wrapper == "confluence"));
        assert!(entries.iter().any(|e| e.principal == "principal-b"
            && e.instance == "robert"
            && e.wrapper == "gh-authorized"));
        assert!(
            !entries
                .iter()
                .any(|e| e.principal == "principal-a" && e.wrapper == "gh-authorized")
        );
    }

    #[test]
    fn admin_ui_overview_counts_resources_wrappers_and_mutation_windows() {
        let store = TestStore::new();
        store.write("gh/main/env/GH_TOKEN", b"gh-token").unwrap();
        store.write("gh/main/files/hosts.json", b"{}").unwrap();
        store
            .write("aws/prod/env/AWS_SECRET", b"aws-secret")
            .unwrap();

        store_catalog_document(
            &store,
            &[
                StoredCatalogEntry {
                    principal: "p1".to_string(),
                    host: "deepthought".to_string(),
                    user: "robert".to_string(),
                    instance: "workstation".to_string(),
                    wrapper: "gh-authorized".to_string(),
                    resource: "gh/main".to_string(),
                },
                StoredCatalogEntry {
                    principal: "p1".to_string(),
                    host: "deepthought".to_string(),
                    user: "robert".to_string(),
                    instance: "workstation".to_string(),
                    wrapper: "aws-prod".to_string(),
                    resource: "aws/prod".to_string(),
                },
                StoredCatalogEntry {
                    principal: "p1".to_string(),
                    host: "deepthought".to_string(),
                    user: "robert".to_string(),
                    instance: "workstation".to_string(),
                    wrapper: "jira-readonly".to_string(),
                    resource: "gh/main".to_string(),
                },
            ],
        )
        .unwrap();

        let doc = PolicyDocument {
            wrappers: vec![
                underwrap_protocol::policy::WrapperPolicyState {
                    wrapper: "gh-authorized".to_string(),
                    mode: WrapperApprovalMode::Locked,
                    reason: None,
                },
                underwrap_protocol::policy::WrapperPolicyState {
                    wrapper: "aws-prod".to_string(),
                    mode: WrapperApprovalMode::PerCallApproval {
                        approval_key: "ssh-ed25519 AAAA".to_string(),
                    },
                    reason: None,
                },
            ],
            rules: vec![underwrap_protocol::policy::PolicyRule {
                id: "rule-1".to_string(),
                wrapper: "gh-authorized".to_string(),
                resource: underwrap_protocol::policy::credential_resource("gh/main/env/GH_TOKEN"),
                operation: underwrap_protocol::policy::OP_STORE_WRITE.to_string(),
                effect: underwrap_protocol::policy::PolicyEffect::Allow,
            }],
            grants: vec![underwrap_protocol::policy::PolicyGrant {
                id: "grant-1".to_string(),
                wrapper: "aws-prod".to_string(),
                resource: underwrap_protocol::policy::credential_resource(
                    "aws/prod/env/AWS_SECRET",
                ),
                operation: underwrap_protocol::policy::OP_STORE_WRITE.to_string(),
                effect: underwrap_protocol::policy::PolicyEffect::Allow,
                starts_at_epoch: Some(100),
                expires_at_epoch: Some(200),
                reason: None,
            }],
        };
        store
            .write(
                POLICY_DOCUMENT_SECRET_ID,
                &serde_json::to_vec(&doc).unwrap(),
            )
            .unwrap();
        append_audit_history_event(
            &store,
            underwrap_protocol::service::AdminUiAuditEvent {
                ts_epoch: 140,
                action: "secret_revealed".to_string(),
                operator: "alice".to_string(),
                wrapper: None,
                resource: Some("gh/main".to_string()),
                secret_id: Some("gh/main/env/GH_TOKEN".to_string()),
                operation: None,
                result: "success".to_string(),
            },
        )
        .unwrap();

        let summary = admin_ui_overview_for_store("work", &store, 150).unwrap();
        assert_eq!(summary.store, "work");
        assert_eq!(summary.resource_count, 2);
        assert_eq!(summary.wrapper_counts.locked, 1);
        assert_eq!(summary.wrapper_counts.per_call_approval, 1);
        assert_eq!(summary.wrapper_counts.unlocked, 1);
        assert_eq!(summary.wrapper_counts.unlocked_until, 0);
        assert_eq!(summary.active_mutation_windows, 2);
        assert_eq!(summary.recent_audit_events, 1);
    }

    #[test]
    fn admin_ui_resources_query_filters_by_query_slot_and_mutation() {
        let store = TestStore::new();
        store.write("gh/main/env/GH_TOKEN", b"gh-token").unwrap();
        store.write("gh/main/files/hosts.json", b"{}").unwrap();
        store
            .write("aws/prod/env/AWS_SECRET", b"aws-secret")
            .unwrap();

        store_catalog_document(
            &store,
            &[
                StoredCatalogEntry {
                    principal: "p1".to_string(),
                    host: "deepthought".to_string(),
                    user: "robert".to_string(),
                    instance: "workstation".to_string(),
                    wrapper: "gh-authorized".to_string(),
                    resource: "gh/main".to_string(),
                },
                StoredCatalogEntry {
                    principal: "p1".to_string(),
                    host: "deepthought".to_string(),
                    user: "robert".to_string(),
                    instance: "workstation".to_string(),
                    wrapper: "aws-prod".to_string(),
                    resource: "aws/prod".to_string(),
                },
            ],
        )
        .unwrap();

        let doc = PolicyDocument {
            wrappers: Vec::new(),
            rules: vec![underwrap_protocol::policy::PolicyRule {
                id: "rule-1".to_string(),
                wrapper: "gh-authorized".to_string(),
                resource: underwrap_protocol::policy::credential_resource("gh/main/env/GH_TOKEN"),
                operation: underwrap_protocol::policy::OP_STORE_WRITE.to_string(),
                effect: underwrap_protocol::policy::PolicyEffect::Allow,
            }],
            grants: Vec::new(),
        };
        store
            .write(
                POLICY_DOCUMENT_SECRET_ID,
                &serde_json::to_vec(&doc).unwrap(),
            )
            .unwrap();

        let response = admin_ui_resources_query(
            &store,
            &underwrap_protocol::service::AdminUiResourcesQueryRequest {
                store: "work".to_string(),
                query: "gh".to_string(),
                slot: Some("files".to_string()),
                mutation_state: Some("active".to_string()),
                page: 1,
                page_size: 50,
            },
            150,
        )
        .unwrap();

        assert_eq!(response.total, 1);
        assert_eq!(response.items.len(), 1);
        let item = &response.items[0];
        assert_eq!(item.resource, "gh/main");
        assert!(item.slots.env);
        assert!(item.slots.files);
        assert!(!item.slots.libsecret);
        assert_eq!(item.catalog_wrappers, vec!["gh-authorized".to_string()]);
        assert!(item.mutation.active);
    }

    #[test]
    fn admin_ui_resource_detail_reports_slot_metadata() {
        let store = TestStore::new();
        store.write("gh/main/env/GH_TOKEN", b"gh-token").unwrap();
        store.write("gh/main/files/hosts.json", b"{}").unwrap();
        store
            .write("gh/main/libsecret/default/item_1", &[0_u8, 1_u8, 2_u8])
            .unwrap();

        let response = admin_ui_resource_detail(&store, "gh/main").unwrap();
        assert_eq!(response.resource, "gh/main");
        assert_eq!(response.slots.env.len(), 1);
        assert_eq!(response.slots.files.len(), 1);
        assert_eq!(response.slots.libsecret.len(), 1);
        assert_eq!(response.slots.env[0].entry, "GH_TOKEN");
        assert!(response.slots.env[0].is_utf8);
        assert_eq!(response.slots.files[0].entry, "hosts.json");
        assert_eq!(
            response.slots.files[0].sha256_hex.as_deref(),
            Some("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a")
        );
        assert_eq!(response.slots.libsecret[0].entry, "default/item_1");
        assert!(!response.slots.libsecret[0].is_utf8);
    }

    #[tokio::test]
    async fn admin_ui_resource_reveal_returns_utf8_and_binary_payloads() {
        let mut stores: HashMap<String, Arc<dyn StorageBackend>> = HashMap::new();
        let store: Arc<dyn StorageBackend> = Arc::new(TestStore::new());
        store.write("gh/main/env/GH_TOKEN", b"gh-token").unwrap();
        store
            .write("gh/main/libsecret/default/item_1", &[0_u8, 1_u8, 2_u8])
            .unwrap();
        stores.insert("work".to_string(), Arc::clone(&store));
        let state = AdminRuntimeState {
            stores,
            tokens: HashMap::new(),
            allowed_origins: None,
        };

        let utf8 = handle_admin_ui_resource_reveal(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiResourceRevealRequest {
                store: "work".to_string(),
                secret_id: "gh/main/env/GH_TOKEN".to_string(),
            },
        );
        assert_eq!(utf8.status(), StatusCode::OK);
        assert_eq!(
            utf8.headers().get(hyper::header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        let utf8_body = utf8.into_body().collect().await.unwrap().to_bytes();
        let utf8_payload: underwrap_protocol::service::AdminUiResourceRevealResponse =
            serde_json::from_slice(&utf8_body).unwrap();
        assert_eq!(
            utf8_payload,
            underwrap_protocol::service::AdminUiResourceRevealResponse::Utf8 {
                text: "gh-token".to_string(),
                size_bytes: 8,
                copy_allowed: true,
            }
        );

        let binary = handle_admin_ui_resource_reveal(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiResourceRevealRequest {
                store: "work".to_string(),
                secret_id: "gh/main/libsecret/default/item_1".to_string(),
            },
        );
        let binary_body = binary.into_body().collect().await.unwrap().to_bytes();
        let binary_payload: underwrap_protocol::service::AdminUiResourceRevealResponse =
            serde_json::from_slice(&binary_body).unwrap();
        assert_eq!(
            binary_payload,
            underwrap_protocol::service::AdminUiResourceRevealResponse::Binary {
                base64: "AAEC".to_string(),
                size_bytes: 3,
                copy_allowed: false,
                download_recommended: true,
            }
        );
    }

    #[tokio::test]
    async fn admin_ui_download_rejects_oversized_payloads_and_sets_download_headers() {
        let mut stores: HashMap<String, Arc<dyn StorageBackend>> = HashMap::new();
        let store: Arc<dyn StorageBackend> = Arc::new(TestStore::new());
        store.write("gh/main/files/hosts.json", b"{}").unwrap();
        store
            .write(
                "gh/main/files/big.bin",
                &vec![0_u8; ADMIN_UI_MAX_FILE_BYTES + 1],
            )
            .unwrap();
        stores.insert("work".to_string(), Arc::clone(&store));
        let state = AdminRuntimeState {
            stores,
            tokens: HashMap::new(),
            allowed_origins: None,
        };

        let ok = handle_admin_ui_resource_download(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiResourceDownloadRequest {
                store: "work".to_string(),
                secret_id: "gh/main/files/hosts.json".to_string(),
            },
        );
        assert_eq!(ok.status(), StatusCode::OK);
        assert_eq!(
            ok.headers().get(hyper::header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        assert_eq!(
            ok.headers()
                .get(hyper::header::CONTENT_DISPOSITION)
                .unwrap(),
            "attachment; filename=\"hosts.json\""
        );
        let ok_body = ok.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(ok_body.as_ref(), b"{}");

        let too_large = handle_admin_ui_resource_download(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiResourceDownloadRequest {
                store: "work".to_string(),
                secret_id: "gh/main/files/big.bin".to_string(),
            },
        );
        assert_eq!(too_large.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn admin_ui_audit_query_filters_and_reveal_download_events_are_queryable() {
        let mut stores: HashMap<String, Arc<dyn StorageBackend>> = HashMap::new();
        let raw_store = Arc::new(TestStore::new());
        raw_store
            .write("gh/main/env/GH_TOKEN", b"gh-token")
            .unwrap();
        raw_store.write("gh/main/files/hosts.json", b"{}").unwrap();
        let store: Arc<dyn StorageBackend> = raw_store.clone();
        stores.insert("work".to_string(), Arc::clone(&store));
        let state = AdminRuntimeState {
            stores,
            tokens: HashMap::new(),
            allowed_origins: None,
        };

        let reveal = handle_admin_ui_resource_reveal(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiResourceRevealRequest {
                store: "work".to_string(),
                secret_id: "gh/main/env/GH_TOKEN".to_string(),
            },
        );
        assert_eq!(reveal.status(), StatusCode::OK);

        let download = handle_admin_ui_resource_download(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiResourceDownloadRequest {
                store: "work".to_string(),
                secret_id: "gh/main/files/hosts.json".to_string(),
            },
        );
        assert_eq!(download.status(), StatusCode::OK);

        append_audit_history_event(
            raw_store.as_ref(),
            underwrap_protocol::service::AdminUiAuditEvent {
                ts_epoch: 50,
                action: "wrapper_mode_updated".to_string(),
                operator: "bob".to_string(),
                wrapper: Some("jira-readonly".to_string()),
                resource: None,
                secret_id: None,
                operation: None,
                result: "success".to_string(),
            },
        )
        .unwrap();

        let all = admin_ui_audit_query(
            raw_store.as_ref(),
            &underwrap_protocol::service::AdminUiAuditQueryRequest {
                store: "work".to_string(),
                operator: None,
                wrapper: None,
                resource: None,
                action: None,
                from_epoch: None,
                to_epoch: None,
                page: 1,
                page_size: 50,
            },
        )
        .unwrap();
        assert_eq!(all.total, 3);
        assert!(
            all.items
                .iter()
                .any(|event| event.action == "secret_revealed")
        );
        assert!(
            all.items
                .iter()
                .any(|event| event.action == "secret_downloaded")
        );

        let filtered = admin_ui_audit_query(
            raw_store.as_ref(),
            &underwrap_protocol::service::AdminUiAuditQueryRequest {
                store: "work".to_string(),
                operator: Some("alice".to_string()),
                wrapper: None,
                resource: Some("gh/main".to_string()),
                action: Some("secret_downloaded".to_string()),
                from_epoch: None,
                to_epoch: None,
                page: 1,
                page_size: 50,
            },
        )
        .unwrap();
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.items[0].action, "secret_downloaded");
        assert_eq!(filtered.items[0].operator, "alice");
        assert_eq!(filtered.items[0].resource.as_deref(), Some("gh/main"));

        let wrapper_filtered = admin_ui_audit_query(
            raw_store.as_ref(),
            &underwrap_protocol::service::AdminUiAuditQueryRequest {
                store: "work".to_string(),
                operator: Some("bob".to_string()),
                wrapper: Some("jira-readonly".to_string()),
                resource: None,
                action: Some("wrapper_mode_updated".to_string()),
                from_epoch: Some(1),
                to_epoch: Some(100),
                page: 1,
                page_size: 50,
            },
        )
        .unwrap();
        assert_eq!(wrapper_filtered.total, 1);
        assert_eq!(
            wrapper_filtered.items[0].wrapper.as_deref(),
            Some("jira-readonly")
        );
    }

    #[test]
    fn admin_ui_mutation_get_groups_resource_state_by_wrapper() {
        let store = TestStore::new();
        store.write("gh/main/env/GH_TOKEN", b"gh-token").unwrap();
        store.write("gh/main/files/hosts.json", b"{}").unwrap();
        store_catalog_document(
            &store,
            &[
                StoredCatalogEntry {
                    principal: "p1".to_string(),
                    host: "deepthought".to_string(),
                    user: "robert".to_string(),
                    instance: "workstation".to_string(),
                    wrapper: "gh-authorized".to_string(),
                    resource: "gh/main".to_string(),
                },
                StoredCatalogEntry {
                    principal: "p1".to_string(),
                    host: "deepthought".to_string(),
                    user: "robert".to_string(),
                    instance: "workstation".to_string(),
                    wrapper: "jira-readonly".to_string(),
                    resource: "gh/main".to_string(),
                },
            ],
        )
        .unwrap();
        let doc = PolicyDocument {
            wrappers: Vec::new(),
            rules: vec![underwrap_protocol::policy::PolicyRule {
                id: "rule-1".to_string(),
                wrapper: "gh-authorized".to_string(),
                resource: underwrap_protocol::policy::credential_resource("gh/main/env/GH_TOKEN"),
                operation: underwrap_protocol::policy::OP_STORE_WRITE.to_string(),
                effect: underwrap_protocol::policy::PolicyEffect::Allow,
            }],
            grants: vec![underwrap_protocol::policy::PolicyGrant {
                id: "grant-1".to_string(),
                wrapper: "jira-readonly".to_string(),
                resource: underwrap_protocol::policy::credential_resource(
                    "gh/main/files/hosts.json",
                ),
                operation: underwrap_protocol::policy::OP_STORE_WRITE.to_string(),
                effect: underwrap_protocol::policy::PolicyEffect::Allow,
                starts_at_epoch: Some(100),
                expires_at_epoch: Some(200),
                reason: None,
            }],
        };
        store
            .write(
                POLICY_DOCUMENT_SECRET_ID,
                &serde_json::to_vec(&doc).unwrap(),
            )
            .unwrap();

        let response = admin_ui_mutation_get(&store, "gh/main", 150).unwrap();
        assert_eq!(response.wrappers.len(), 2);
        assert_eq!(response.wrappers[0].wrapper, "gh-authorized");
        assert_eq!(response.wrappers[0].mode, "permanent");
        assert!(response.wrappers[0].active);
        assert_eq!(response.wrappers[1].wrapper, "jira-readonly");
        assert_eq!(response.wrappers[1].mode, "temporary");
        assert_eq!(response.wrappers[1].expires_at_epoch, Some(200));
    }

    #[test]
    fn admin_ui_wrappers_query_filters_by_mode_and_query() {
        let store = TestStore::new();
        store_catalog_document(
            &store,
            &[
                StoredCatalogEntry {
                    principal: "p1".to_string(),
                    host: "deepthought".to_string(),
                    user: "robert".to_string(),
                    instance: "workstation".to_string(),
                    wrapper: "gh-authorized".to_string(),
                    resource: "gh/main".to_string(),
                },
                StoredCatalogEntry {
                    principal: "p1".to_string(),
                    host: "deepthought".to_string(),
                    user: "robert".to_string(),
                    instance: "workstation".to_string(),
                    wrapper: "jira-readonly".to_string(),
                    resource: "jira/main".to_string(),
                },
            ],
        )
        .unwrap();
        let doc = PolicyDocument {
            wrappers: vec![
                underwrap_protocol::policy::WrapperPolicyState {
                    wrapper: "gh-authorized".to_string(),
                    mode: WrapperApprovalMode::PerCallApproval {
                        approval_key: "ssh-ed25519 AAAA".to_string(),
                    },
                    reason: Some("ops approval".to_string()),
                },
                underwrap_protocol::policy::WrapperPolicyState {
                    wrapper: "jira-readonly".to_string(),
                    mode: WrapperApprovalMode::Locked,
                    reason: Some("freeze".to_string()),
                },
            ],
            rules: Vec::new(),
            grants: Vec::new(),
        };
        store
            .write(
                POLICY_DOCUMENT_SECRET_ID,
                &serde_json::to_vec(&doc).unwrap(),
            )
            .unwrap();

        let response = admin_ui_wrappers_query(
            &store,
            &underwrap_protocol::service::AdminUiWrappersQueryRequest {
                store: "work".to_string(),
                query: "gh".to_string(),
                mode: Some("per_call_approval".to_string()),
            },
        )
        .unwrap();
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].wrapper, "gh-authorized");
        assert_eq!(response.items[0].resource_count, 1);
        assert_eq!(response.items[0].reason.as_deref(), Some("ops approval"));
    }

    #[tokio::test]
    async fn admin_ui_wrapper_set_mode_updates_policy_state() {
        let mut stores: HashMap<String, Arc<dyn StorageBackend>> = HashMap::new();
        let raw_store = Arc::new(TestStore::new());
        let store: Arc<dyn StorageBackend> = raw_store.clone();
        stores.insert("work".to_string(), Arc::clone(&store));
        let state = AdminRuntimeState {
            stores,
            tokens: HashMap::new(),
            allowed_origins: None,
        };

        let response = handle_admin_ui_wrapper_set_mode(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiWrapperSetModeRequest {
                store: "work".to_string(),
                wrapper: "gh-authorized".to_string(),
                mode: WrapperApprovalMode::Unlocked {
                    until_epoch: Some(1_760_000_000),
                },
                reason: Some("maintenance window".to_string()),
            },
        );
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let payload: underwrap_protocol::service::AdminUiWrapperSetModeResponse =
            serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.wrapper, "gh-authorized");
        assert_eq!(payload.reason.as_deref(), Some("maintenance window"));

        let policy = read_policy(&raw_store);
        assert_eq!(policy.wrappers.len(), 1);
        assert_eq!(policy.wrappers[0].wrapper, "gh-authorized");
        assert_eq!(
            policy.wrappers[0].reason.as_deref(),
            Some("maintenance window")
        );
        assert_eq!(
            policy.wrappers[0].mode,
            WrapperApprovalMode::Unlocked {
                until_epoch: Some(1_760_000_000),
            }
        );
    }

    #[test]
    fn admin_ui_matrix_query_returns_catalog_rows_and_policy_cells() {
        let store = TestStore::new();
        store_catalog_document(
            &store,
            &[
                StoredCatalogEntry {
                    principal: "p1".to_string(),
                    host: "deepthought".to_string(),
                    user: "robert".to_string(),
                    instance: "workstation".to_string(),
                    wrapper: "gh-authorized".to_string(),
                    resource: "gh/main".to_string(),
                },
                StoredCatalogEntry {
                    principal: "p1".to_string(),
                    host: "smallthought".to_string(),
                    user: "robert".to_string(),
                    instance: "workstation".to_string(),
                    wrapper: "gh-authorized".to_string(),
                    resource: "gh/main".to_string(),
                },
            ],
        )
        .unwrap();
        let doc = PolicyDocument {
            wrappers: Vec::new(),
            rules: vec![underwrap_protocol::policy::PolicyRule {
                id: "rule-1".to_string(),
                wrapper: "gh-authorized".to_string(),
                resource: "env:gh/main".to_string(),
                operation: underwrap_protocol::policy::OP_ENV_READ.to_string(),
                effect: underwrap_protocol::policy::PolicyEffect::Allow,
            }],
            grants: Vec::new(),
        };
        store
            .write(
                POLICY_DOCUMENT_SECRET_ID,
                &serde_json::to_vec(&doc).unwrap(),
            )
            .unwrap();

        let response = admin_ui_matrix_query(
            &store,
            &underwrap_protocol::service::AdminUiMatrixQueryRequest {
                store: "work".to_string(),
                resource: Some("gh/main".to_string()),
                host: Some("deepthought".to_string()),
                user: None,
                wrapper: None,
            },
            150,
        )
        .unwrap();
        assert_eq!(response.rows.len(), 1);
        assert_eq!(response.rows[0].host, "deepthought");
        assert_eq!(response.rows[0].wrapper, "gh-authorized");
        assert!(response.rows[0].cells[underwrap_protocol::policy::OP_ENV_READ]);
        assert!(!response.rows[0].cells[underwrap_protocol::policy::OP_FILES_READ]);
    }

    #[tokio::test]
    async fn admin_ui_matrix_set_cell_upserts_and_deletes_allow_rule() {
        let mut stores: HashMap<String, Arc<dyn StorageBackend>> = HashMap::new();
        let raw_store = Arc::new(TestStore::new());
        write_policy(&raw_store, &PolicyDocument::default());
        let store: Arc<dyn StorageBackend> = raw_store.clone();
        stores.insert("work".to_string(), Arc::clone(&store));
        let state = AdminRuntimeState {
            stores,
            tokens: HashMap::new(),
            allowed_origins: None,
        };

        let enabled = handle_admin_ui_matrix_set_cell(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiMatrixSetCellRequest {
                store: "work".to_string(),
                wrapper: "gh-authorized".to_string(),
                resource: "gh/main".to_string(),
                operation: underwrap_protocol::policy::OP_ENV_READ.to_string(),
                enabled: true,
            },
        );
        assert_eq!(enabled.status(), StatusCode::OK);
        let policy = read_policy(&raw_store);
        assert_eq!(policy.rules.len(), 1);
        assert_eq!(policy.rules[0].wrapper, "gh-authorized");
        assert_eq!(policy.rules[0].resource, "env:gh/main");
        assert_eq!(
            policy.rules[0].operation,
            underwrap_protocol::policy::OP_ENV_READ
        );

        let disabled = handle_admin_ui_matrix_set_cell(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiMatrixSetCellRequest {
                store: "work".to_string(),
                wrapper: "gh-authorized".to_string(),
                resource: "gh/main".to_string(),
                operation: underwrap_protocol::policy::OP_ENV_READ.to_string(),
                enabled: false,
            },
        );
        assert_eq!(disabled.status(), StatusCode::OK);
        assert!(read_policy(&raw_store).rules.is_empty());
    }

    #[tokio::test]
    async fn admin_ui_mutation_set_write_and_disable_roundtrip() {
        let mut stores: HashMap<String, Arc<dyn StorageBackend>> = HashMap::new();
        let raw_store = Arc::new(TestStore::new());
        write_policy(&raw_store, &PolicyDocument::default());
        let store: Arc<dyn StorageBackend> = raw_store.clone();
        stores.insert("work".to_string(), Arc::clone(&store));
        let state = AdminRuntimeState {
            stores,
            tokens: HashMap::new(),
            allowed_origins: None,
        };

        let denied = handle_admin_ui_resource_write_text(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiResourceWriteTextRequest {
                store: "work".to_string(),
                wrapper: "gh-authorized".to_string(),
                secret_id: "gh/main/env/GH_TOKEN".to_string(),
                material_utf8: "ghp-before".to_string(),
            },
            100,
        );
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        let denied_body = denied.into_body().collect().await.unwrap().to_bytes();
        let denied_error: underwrap_protocol::service::ServiceErrorResponse =
            serde_json::from_slice(&denied_body).unwrap();
        assert_eq!(denied_error.code, "mutation_denied");

        let enabled = handle_admin_ui_mutation_set(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiMutationSetRequest {
                store: "work".to_string(),
                wrapper: "gh-authorized".to_string(),
                secret_id: "gh/main/env/GH_TOKEN".to_string(),
                mode: underwrap_protocol::service::AdminUiMutationSetMode::Temporary,
                expires_at_epoch: Some(200),
            },
            100,
        );
        assert_eq!(enabled.status(), StatusCode::OK);
        let enabled_body = enabled.into_body().collect().await.unwrap().to_bytes();
        let enabled_payload: underwrap_protocol::service::AdminUiMutationSetResponse =
            serde_json::from_slice(&enabled_body).unwrap();
        assert!(enabled_payload.active);
        assert_eq!(enabled_payload.expires_at_epoch, Some(200));

        let stored = handle_admin_ui_resource_write_text(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiResourceWriteTextRequest {
                store: "work".to_string(),
                wrapper: "gh-authorized".to_string(),
                secret_id: "gh/main/env/GH_TOKEN".to_string(),
                material_utf8: "ghp-after".to_string(),
            },
            120,
        );
        assert_eq!(stored.status(), StatusCode::OK);
        assert_eq!(
            raw_store
                .read("gh/main/env/GH_TOKEN")
                .unwrap()
                .unwrap()
                .as_ref(),
            b"ghp-after"
        );

        let disabled = handle_admin_ui_mutation_set(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiMutationSetRequest {
                store: "work".to_string(),
                wrapper: "gh-authorized".to_string(),
                secret_id: "gh/main/env/GH_TOKEN".to_string(),
                mode: underwrap_protocol::service::AdminUiMutationSetMode::Disabled,
                expires_at_epoch: None,
            },
            130,
        );
        assert_eq!(disabled.status(), StatusCode::OK);

        let denied_again = handle_admin_ui_resource_write_text(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiResourceWriteTextRequest {
                store: "work".to_string(),
                wrapper: "gh-authorized".to_string(),
                secret_id: "gh/main/env/GH_TOKEN".to_string(),
                material_utf8: "ghp-final".to_string(),
            },
            140,
        );
        assert_eq!(denied_again.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_ui_file_upload_and_delete_enforce_mutation_and_size_limit() {
        let mut stores: HashMap<String, Arc<dyn StorageBackend>> = HashMap::new();
        let raw_store = Arc::new(TestStore::new());
        write_policy(&raw_store, &PolicyDocument::default());
        let store: Arc<dyn StorageBackend> = raw_store.clone();
        stores.insert("work".to_string(), Arc::clone(&store));
        let state = AdminRuntimeState {
            stores,
            tokens: HashMap::new(),
            allowed_origins: None,
        };

        let denied_upload = handle_admin_ui_resource_upload_file(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiResourceUploadFileRequest {
                store: "work".to_string(),
                wrapper: "gh-authorized".to_string(),
                secret_id: "gh/main/files/hosts.json".to_string(),
                material_b64: base64::engine::general_purpose::STANDARD.encode(b"{}"),
            },
            100,
        );
        assert_eq!(denied_upload.status(), StatusCode::FORBIDDEN);

        let enabled = handle_admin_ui_mutation_set(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiMutationSetRequest {
                store: "work".to_string(),
                wrapper: "gh-authorized".to_string(),
                secret_id: "gh/main/files/hosts.json".to_string(),
                mode: underwrap_protocol::service::AdminUiMutationSetMode::Temporary,
                expires_at_epoch: Some(200),
            },
            100,
        );
        assert_eq!(enabled.status(), StatusCode::OK);

        let uploaded = handle_admin_ui_resource_upload_file(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiResourceUploadFileRequest {
                store: "work".to_string(),
                wrapper: "gh-authorized".to_string(),
                secret_id: "gh/main/files/hosts.json".to_string(),
                material_b64: base64::engine::general_purpose::STANDARD.encode(b"{}"),
            },
            110,
        );
        assert_eq!(uploaded.status(), StatusCode::OK);

        let deleted = handle_admin_ui_resource_delete_entry(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiResourceDeleteEntryRequest {
                store: "work".to_string(),
                wrapper: "gh-authorized".to_string(),
                secret_id: "gh/main/files/hosts.json".to_string(),
            },
            120,
        );
        assert_eq!(deleted.status(), StatusCode::OK);
        let deleted_body = deleted.into_body().collect().await.unwrap().to_bytes();
        let deleted_payload: underwrap_protocol::service::AdminUiResourceDeleteEntryResponse =
            serde_json::from_slice(&deleted_body).unwrap();
        assert!(deleted_payload.existed);

        let too_large = handle_admin_ui_resource_upload_file(
            &state,
            "alice",
            underwrap_protocol::service::AdminUiResourceUploadFileRequest {
                store: "work".to_string(),
                wrapper: "gh-authorized".to_string(),
                secret_id: "gh/main/files/big.bin".to_string(),
                material_b64: base64::engine::general_purpose::STANDARD.encode(vec![
                    0_u8;
                    ADMIN_UI_MAX_FILE_BYTES
                        + 1
                ]),
            },
            130,
        );
        assert_eq!(too_large.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    fn admin_state_with_tokens(tokens: &[(&str, &str)]) -> AdminRuntimeState {
        AdminRuntimeState {
            stores: HashMap::new(),
            tokens: tokens
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            allowed_origins: None,
        }
    }

    fn headers_with_auth(value: Option<&str>) -> hyper::HeaderMap {
        let mut headers = hyper::HeaderMap::new();
        if let Some(v) = value {
            headers.insert(
                hyper::header::AUTHORIZATION,
                hyper::header::HeaderValue::from_str(v).unwrap(),
            );
        }
        headers
    }

    #[test]
    fn authenticate_admin_rejects_missing_and_malformed_headers() {
        let state = admin_state_with_tokens(&[("alice", "secret-alice-token-1234")]);
        assert!(authenticate_admin(&state, &headers_with_auth(None)).is_err());
        assert!(authenticate_admin(&state, &headers_with_auth(Some("Basic abc"))).is_err());
        assert!(authenticate_admin(&state, &headers_with_auth(Some("Bearer "))).is_err());
    }

    #[test]
    fn authenticate_admin_accepts_matching_token_and_rejects_others() {
        let state = admin_state_with_tokens(&[
            ("alice", "secret-alice-token-1234"),
            ("bob", "secret-bob-token-abcd"),
        ]);
        let ok = authenticate_admin(
            &state,
            &headers_with_auth(Some("Bearer secret-bob-token-abcd")),
        )
        .unwrap();
        assert_eq!(ok, "bob");

        assert!(
            authenticate_admin(&state, &headers_with_auth(Some("Bearer unknown-token"))).is_err()
        );
        assert!(
            authenticate_admin(
                &state,
                &headers_with_auth(Some("Bearer secret-alice-token-12345")), // off by one byte
            )
            .is_err()
        );
    }

    #[test]
    fn load_admin_tokens_reads_trimmed_token_from_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("tok");
        std::fs::write(&path, "  hello-token-value  \n").unwrap();
        let tokens = load_admin_tokens(&HashMap::from([(
            "alice".to_string(),
            AdminTokenConfig { token_file: path },
        )]))
        .unwrap();
        assert_eq!(tokens.get("alice").unwrap(), "hello-token-value");
    }

    #[test]
    fn load_admin_tokens_rejects_empty_token() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("tok");
        std::fs::write(&path, "   \n").unwrap();
        let err = load_admin_tokens(&HashMap::from([(
            "alice".to_string(),
            AdminTokenConfig { token_file: path },
        )]))
        .unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn is_loopback_listen_recognises_loopback_addresses() {
        assert!(is_loopback_listen("127.0.0.1:8080"));
        assert!(is_loopback_listen("127.0.0.5:0"));
        assert!(is_loopback_listen("[::1]:8080"));
        assert!(is_loopback_listen("localhost:443"));
        assert!(is_loopback_listen("LOCALHOST:443"));
    }

    #[test]
    fn is_loopback_listen_rejects_non_loopback_addresses() {
        assert!(!is_loopback_listen("0.0.0.0:8080"));
        assert!(!is_loopback_listen("[::]:8080"));
        assert!(!is_loopback_listen("192.168.1.10:8080"));
        assert!(!is_loopback_listen("10.0.0.1:443"));
        assert!(!is_loopback_listen("example.com:443"));
        assert!(!is_loopback_listen("8080")); // no colon at all
        assert!(!is_loopback_listen(":8080")); // empty host
    }

    #[test]
    fn validate_admin_config_allows_loopback_plain_http() {
        let cfg = AdminConfig {
            listen: "127.0.0.1:19443".to_string(),
            tokens: HashMap::new(),
            tls: None,
            allowed_origins: None,
        };
        validate_admin_config(&cfg).expect("loopback plain HTTP must be allowed");
    }

    #[test]
    fn validate_admin_config_allows_non_loopback_with_tls() {
        let cfg = AdminConfig {
            listen: "0.0.0.0:18444".to_string(),
            tokens: HashMap::new(),
            tls: Some(AdminTlsConfig {
                cert_file: PathBuf::from("/dev/null"),
                key_file: PathBuf::from("/dev/null"),
            }),
            allowed_origins: None,
        };
        validate_admin_config(&cfg).expect("TLS on non-loopback must be allowed");
    }

    #[test]
    fn validate_admin_config_refuses_non_loopback_plain_http() {
        let cfg = AdminConfig {
            listen: "0.0.0.0:18444".to_string(),
            tokens: HashMap::new(),
            tls: None,
            allowed_origins: None,
        };
        let err = validate_admin_config(&cfg).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("not loopback") && msg.contains("TLS"),
            "expected fail-closed message, got: {msg}"
        );
    }

    #[test]
    fn validate_admin_config_rejects_empty_origin_entry() {
        let cfg = AdminConfig {
            listen: "127.0.0.1:19443".to_string(),
            tokens: HashMap::new(),
            tls: None,
            allowed_origins: Some(vec!["https://ok.example".to_string(), "  ".to_string()]),
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
}
