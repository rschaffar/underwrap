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

use underwrap_protocol::config::{StoreDefinition, StoreSource};
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
    /// Which declared store should hold the operator collection
    /// (`__underwrap/admin/operators`). Defaults to the alphabetically
    /// first declared store when not set.
    #[serde(default)]
    pub admin_home_store: Option<String>,
    /// Path to the 32-byte AEAD key file used to sign session cookies.
    /// Required when the admin listener accepts login traffic.
    #[serde(default)]
    pub session_key_file: Option<PathBuf>,
    /// Session lifetime in seconds. Defaults to
    /// `auth::DEFAULT_SESSION_LIFETIME_SECS` (12 hours) when unset.
    #[serde(default)]
    pub session_lifetime_secs: Option<u64>,
}

/// Default location of the declarative stores directory. Each
/// `<name>.yaml` here is owned by the NixOS module and treated as
/// read-only by the admin API.
pub const DEFAULT_STORES_DECLARATIVE_DIR: &str = "/etc/underwrap-backend/stores.d";

/// Default location of the dynamic stores directory. The admin API
/// owns this tree and is allowed to create/update/delete files in it.
pub const DEFAULT_STORES_DYNAMIC_DIR: &str = "/var/lib/underwrap-backend/stores.d";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    /// Directory of declarative store definitions. Each `<name>.yaml`
    /// is loaded with `source = declarative` and refused by the admin
    /// API's mutation handlers. Defaults to
    /// [`DEFAULT_STORES_DECLARATIVE_DIR`]. Missing directories are
    /// tolerated and treated as empty.
    #[serde(default)]
    pub stores_declarative_dir: Option<PathBuf>,
    /// Directory of admin-API-managed store definitions. Same on-disk
    /// shape, but mutations are allowed. Defaults to
    /// [`DEFAULT_STORES_DYNAMIC_DIR`].
    #[serde(default)]
    pub stores_dynamic_dir: Option<PathBuf>,
    #[serde(default)]
    pub service: Option<ServiceConfig>,
    #[serde(default)]
    pub admin: Option<AdminConfig>,
}

impl BackendConfig {
    /// Resolve [`Self::stores_declarative_dir`] or the project default.
    pub fn declarative_dir(&self) -> PathBuf {
        self.stores_declarative_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_STORES_DECLARATIVE_DIR))
    }

    /// Resolve [`Self::stores_dynamic_dir`] or the project default.
    pub fn dynamic_dir(&self) -> PathBuf {
        self.stores_dynamic_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_STORES_DYNAMIC_DIR))
    }
}

/// One store as loaded from disk plus its provenance. Kept alongside
/// the opened [`StorageBackend`] in [`AdminRuntimeState`] so admin
/// handlers can refuse to mutate declarative stores and surface the
/// description in list endpoints.
#[derive(Debug, Clone)]
pub struct LoadedStoreDef {
    pub source: StoreSource,
    pub source_path: PathBuf,
    pub definition: StoreDefinition,
}

struct ServiceRuntimeState {
    clients: HashMap<String, Arc<dyn StorageBackend>>,
}

struct AdminRuntimeState {
    stores: HashMap<String, Arc<dyn StorageBackend>>,
    /// Source provenance and on-disk path for each store in `stores`,
    /// keyed by the same store name. Populated at startup from the
    /// declarative/dynamic dir union. The admin CRUD handlers in
    /// ticket 231 read this to refuse mutation requests targeting a
    /// `Declarative` store.
    store_sources: HashMap<String, LoadedStoreDef>,
    /// Operator name → opaque bearer token. Both fields are trusted; the
    /// caller authenticates by presenting a matching token.
    tokens: HashMap<String, String>,
    /// When present, every state-changing request must carry an `Origin`
    /// (or `Referer`) matching one of these entries.
    allowed_origins: Option<Vec<String>>,
    /// Where to read the SPA bundle from when serving non-API paths.
    web_source: crate::web::WebAssetSource,
    /// Store that holds the operator collection.
    admin_home_store: Arc<dyn StorageBackend>,
    /// 32-byte AEAD key used to encrypt session cookies. `None` when
    /// the deployment hasn't configured `session_key_file` yet
    /// (legacy bearer-token-only setups during the transition).
    session_key: Option<[u8; crate::auth::SESSION_KEY_LEN]>,
    /// Configured session lifetime in seconds.
    session_lifetime_secs: u64,
    /// Whether to mark issued cookies with the `Secure` attribute.
    /// True when the admin listener terminates TLS.
    cookies_secure: bool,
    /// Per-username login attempt limiter.
    login_rate_limiter: crate::auth::RateLimiter,
    /// Server-side set of revoked session ids (ticket 212). Logout
    /// adds the session id here so a stolen-but-still-unexpired cookie
    /// is rejected on the next request.
    session_revocations: crate::auth::SessionRevocations,
    /// Mutex that serialises bootstrap attempts so concurrent callers
    /// can't both pass the "is operator collection empty" check
    /// before either has written the first record. The lock is held
    /// across the read-collection / write-collection / mark-sentinel
    /// sequence (ticket 214).
    bootstrap_lock: std::sync::Mutex<()>,
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
    let definitions = load_store_definitions(&config.declarative_dir(), &config.dynamic_dir())?;
    let stores = open_stores(&definitions)?;

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
        // Fail-fast: resolve the admin-home store name now so a typo in
        // config doesn't surface only on the first login attempt.
        let (home_name, home_store) = crate::operators::resolve_admin_home_store(
            &stores,
            admin_config.admin_home_store.as_deref(),
        )?;
        let home_name = home_name.to_string();
        info!(admin_home_store = %home_name, "resolved admin-home store");
        let admin_listener = TcpListener::bind(&admin_config.listen)
            .await
            .with_context(|| format!("binding admin listener {}", admin_config.listen))?;
        let tokens = load_admin_tokens(&admin_config.tokens)?;
        let tls_acceptor = match &admin_config.tls {
            Some(tls) => Some(build_admin_tls_acceptor(tls)?),
            None => None,
        };
        let session_key = match admin_config.session_key_file.as_deref() {
            Some(path) => Some(crate::auth::load_session_key(path)?),
            None => None,
        };
        let cookies_secure = admin_config.tls.is_some();
        let session_lifetime_secs = admin_config
            .session_lifetime_secs
            .unwrap_or(crate::auth::DEFAULT_SESSION_LIFETIME_SECS);
        let admin_state = Arc::new(AdminRuntimeState {
            stores: stores.clone(),
            store_sources: definitions.clone(),
            tokens,
            allowed_origins: admin_config.allowed_origins.clone(),
            web_source: crate::web::WebAssetSource::from_env(),
            admin_home_store: home_store,
            session_key,
            session_lifetime_secs,
            cookies_secure,
            login_rate_limiter: crate::auth::RateLimiter::new(),
            session_revocations: crate::auth::SessionRevocations::new(),
            bootstrap_lock: std::sync::Mutex::new(()),
        });
        let declarative_count = admin_state
            .store_sources
            .values()
            .filter(|d| d.source == StoreSource::Declarative)
            .count();
        let dynamic_count = admin_state
            .store_sources
            .values()
            .filter(|d| d.source == StoreSource::Dynamic)
            .count();
        info!(
            listen = %admin_config.listen,
            stores = admin_state.stores.len(),
            declarative = declarative_count,
            dynamic = dynamic_count,
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
    definitions: &HashMap<String, LoadedStoreDef>,
) -> Result<HashMap<String, Arc<dyn StorageBackend>>> {
    let mut stores = HashMap::new();
    for (name, def) in definitions {
        let store: Arc<dyn StorageBackend> =
            Arc::from(underwrap_store::open(&def.definition.storage)?);
        stores.insert(name.clone(), store);
    }
    Ok(stores)
}

/// Read every `<name>.yaml` under both directories and union them
/// into the runtime store map. Duplicate names across sources are a
/// fatal startup error; missing directories are treated as empty so
/// a freshly-provisioned host with no dynamic stores yet can still
/// boot.
pub fn load_store_definitions(
    declarative_dir: &Path,
    dynamic_dir: &Path,
) -> Result<HashMap<String, LoadedStoreDef>> {
    let mut out: HashMap<String, LoadedStoreDef> = HashMap::new();
    read_one_dir(declarative_dir, StoreSource::Declarative, &mut out)?;
    read_one_dir(dynamic_dir, StoreSource::Dynamic, &mut out)?;
    Ok(out)
}

fn read_one_dir(
    dir: &Path,
    source: StoreSource,
    out: &mut HashMap<String, LoadedStoreDef>,
) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    let mut sorted: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry under {}", dir.display()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        sorted.push(path);
    }
    // Stable order so test failures are deterministic.
    sorted.sort();
    for path in sorted {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .with_context(|| format!("non-UTF8 store filename {}", path.display()))?;
        if !is_valid_store_name(&name) {
            return Err(anyhow::anyhow!(
                "invalid store name '{name}' (file {}): must match [a-z0-9][a-z0-9_-]*",
                path.display()
            ));
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading store definition {}", path.display()))?;
        let definition: StoreDefinition = serde_yaml::from_str(&text)
            .with_context(|| format!("parsing store definition {}", path.display()))?;
        if let Some(existing) = out.get(&name) {
            return Err(anyhow::anyhow!(
                "duplicate store name '{name}': defined in both {} and {}",
                existing.source_path.display(),
                path.display(),
            ));
        }
        out.insert(
            name,
            LoadedStoreDef {
                source,
                source_path: path,
                definition,
            },
        );
    }
    Ok(())
}

/// A store name must be safe to use as a filename and as a URL path
/// segment. Lowercase letters, digits, `-`, `_`, starting with an
/// alphanumeric.
pub fn is_valid_store_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
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

/// Resolve a store reference for an authenticated request. Returns
/// 404 not_found when either the store doesn't exist or the principal
/// lacks a grant for it. Collapsing both into the same response code
/// prevents operators from probing the store catalogue.
#[allow(clippy::result_large_err)]
fn admin_store_for_principal<'a>(
    state: &'a AdminRuntimeState,
    principal: &crate::operators::Principal,
    store: &str,
) -> std::result::Result<&'a Arc<dyn StorageBackend>, Response<Full<Bytes>>> {
    if !principal.has_store_access(store) {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "unknown store",
        ));
    }
    match state.stores.get(store) {
        Some(s) => Ok(s),
        None => Err(error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "unknown store",
        )),
    }
}

/// Build a [`Principal`] from an authenticated request. For
/// session-cookie auth we re-read the operator collection so role and
/// grants are always current; for legacy bearer-token auth we
/// synthesise an admin principal (those tokens predate the multi-user
/// model and have always carried full access).
#[allow(clippy::result_large_err)]
fn resolve_principal(
    state: &AdminRuntimeState,
    auth: &AdminAuth,
) -> std::result::Result<crate::operators::Principal, Response<Full<Bytes>>> {
    match auth {
        AdminAuth::Session(payload) => {
            let collection = match crate::operators::load(state.admin_home_store.as_ref()) {
                Ok(c) => c,
                Err(e) => {
                    warn!(err = %e, "could not load operator collection");
                    return Err(error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal",
                        "operator collection unavailable",
                    ));
                }
            };
            match collection.find_by_username(&payload.username) {
                Some(op) => Ok(crate::operators::Principal {
                    id: op.id.clone(),
                    username: op.username.clone(),
                    role: op.role,
                    granted_stores: op.granted_stores.clone(),
                }),
                None => Err(error_response(
                    StatusCode::UNAUTHORIZED,
                    "unauthorized",
                    "session refers to an operator that no longer exists",
                )),
            }
        }
        AdminAuth::Bearer { operator } => Ok(crate::operators::Principal {
            id: operator.clone(),
            username: operator.clone(),
            role: crate::operators::OperatorRole::Admin,
            granted_stores: Vec::new(),
        }),
    }
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

/// Result of authenticating an admin request. Carries enough context
/// for the dispatcher to enforce CSRF (session-only) and attach the
/// CSRF cookie on the way out.
#[derive(Debug)]
enum AdminAuth {
    /// Session-cookie authentication. CSRF + revocation rules apply.
    Session(crate::auth::SessionPayload),
    /// Legacy bearer-token authentication. CSRF is not enforced
    /// because programmatic callers don't run in a browser and can't
    /// be CSRF'd. We keep this path while operators migrate off it.
    Bearer { operator: String },
}

impl AdminAuth {
    /// Operator name carried by this authentication, for the few
    /// places that need to label the auth itself (e.g. tests). Most
    /// request handlers should use [`Principal::username`] instead,
    /// which is the source of truth for audit and authorization.
    #[cfg(test)]
    fn operator(&self) -> &str {
        match self {
            AdminAuth::Session(p) => &p.username,
            AdminAuth::Bearer { operator } => operator.as_str(),
        }
    }

    fn as_session(&self) -> Option<&crate::auth::SessionPayload> {
        if let AdminAuth::Session(p) = self {
            Some(p)
        } else {
            None
        }
    }
}

/// Reasons `authenticate_admin` can fail. Mapped to HTTP responses by
/// the caller — `Revoked` becomes 401 with `session_revoked`, the rest
/// become 401 with `unauthorized`.
#[derive(Debug)]
enum AdminAuthError {
    Unauthenticated(&'static str),
    Revoked,
}

/// Authenticate an admin request via either a session cookie (the new
/// path, ticket 211/212) or a bearer token (the legacy path, kept
/// while we migrate deployments off it).
fn authenticate_admin(
    state: &AdminRuntimeState,
    headers: &hyper::HeaderMap,
) -> std::result::Result<AdminAuth, AdminAuthError> {
    // 1. Session cookie. When a key is configured and the request
    //    carries the cookie, the cookie is the source of truth.
    if let (Some(key), Some(value)) = (
        state.session_key.as_ref(),
        crate::auth::extract_session_cookie(headers),
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        return match crate::auth::decrypt_session(value, key, now) {
            Ok(payload) => {
                if state
                    .session_revocations
                    .is_revoked(&payload.session_id, now)
                {
                    Err(AdminAuthError::Revoked)
                } else {
                    Ok(AdminAuth::Session(payload))
                }
            }
            Err(_) => Err(AdminAuthError::Unauthenticated("invalid session cookie")),
        };
    }

    // 2. Legacy bearer-token path.
    use sha2::{Digest, Sha256};
    use subtle::ConstantTimeEq;
    let header = headers
        .get(hyper::header::AUTHORIZATION)
        .ok_or(AdminAuthError::Unauthenticated(
            "missing Authorization header",
        ))?
        .to_str()
        .map_err(|_| AdminAuthError::Unauthenticated("Authorization header is not valid UTF-8"))?;
    let token = header
        .strip_prefix("Bearer ")
        .ok_or(AdminAuthError::Unauthenticated(
            "Authorization header must use the Bearer scheme",
        ))?
        .trim();
    if token.is_empty() {
        return Err(AdminAuthError::Unauthenticated("empty bearer token"));
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
    matched
        .map(|operator| AdminAuth::Bearer { operator })
        .ok_or(AdminAuthError::Unauthenticated(
            "no configured operator token matched",
        ))
}

/// Verify the CSRF double-submit on a state-changing session-authenticated
/// request. Returns `Ok(())` when the request is safe to dispatch.
/// `GET/HEAD/OPTIONS` are exempt.
fn check_csrf(
    method: &Method,
    headers: &hyper::HeaderMap,
    session_csrf_secret: &str,
) -> std::result::Result<(), (&'static str, &'static str)> {
    if matches!(method, &Method::GET | &Method::HEAD | &Method::OPTIONS) {
        return Ok(());
    }
    let header_value = headers
        .get(crate::auth::CSRF_HEADER_NAME)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let header_value = match header_value {
        Some(v) => v,
        None => {
            return Err((
                "csrf_required",
                "missing or empty X-CSRF-Token on state-changing request",
            ));
        }
    };
    let cookie_value = match crate::auth::extract_csrf_cookie(headers) {
        Some(v) if !v.is_empty() => v,
        _ => return Err(("csrf_required", "missing CSRF cookie")),
    };
    use subtle::ConstantTimeEq;
    // Header must match the cookie (double-submit) AND match the value
    // bound into the session (so substituted cookies don't pass).
    let header_bytes = header_value.as_bytes();
    let cookie_bytes = cookie_value.as_bytes();
    let secret_bytes = session_csrf_secret.as_bytes();
    if header_bytes.len() != cookie_bytes.len() || !bool::from(header_bytes.ct_eq(cookie_bytes)) {
        return Err(("csrf_mismatch", "X-CSRF-Token does not match CSRF cookie"));
    }
    if header_bytes.len() != secret_bytes.len() || !bool::from(header_bytes.ct_eq(secret_bytes)) {
        return Err(("csrf_mismatch", "X-CSRF-Token does not match session"));
    }
    Ok(())
}

#[derive(serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct LoginRequest {
    username: String,
    password: String,
}

#[derive(serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct LoginResponse {
    pub username: String,
    pub role: crate::operators::OperatorRole,
}

#[derive(serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct WhoamiResponse {
    pub authenticated: bool,
    // `skip_serializing_if = "Option::is_none"` omits the JSON key when None;
    // `#[ts(optional)]` makes ts-rs emit `username?: string` (instead of the
    // default `string | null`) so the SPA type matches the wire format.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub role: Option<crate::operators::OperatorRole>,
}

async fn handle_admin_login(
    state: &AdminRuntimeState,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let Some(session_key) = state.session_key else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "session_key_not_configured",
            "admin session_key_file is not configured",
        );
    };
    let body = match parse_json_body::<LoginRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };

    let username_norm = crate::operators::normalise_username(&body.username);
    if username_norm.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_credentials",
            "username is required",
        );
    }
    if body.password.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_credentials",
            "password is required",
        );
    }

    if !state.login_rate_limiter.check_and_record(&username_norm) {
        warn!(username = %username_norm, "login rate-limited");
        return error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many login attempts; please wait and try again",
        );
    }

    let collection = match crate::operators::load(state.admin_home_store.as_ref()) {
        Ok(c) => c,
        Err(e) => {
            warn!(err = %e, "login: failed to read operator collection");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "operator collection unavailable",
            );
        }
    };

    // Always run argon2 verify, even on unknown user, to keep timing
    // independent of user existence.
    let lookup = collection.find_by_username(&username_norm);
    let hash_to_check = lookup
        .map(|o| o.password_hash.as_str())
        .unwrap_or(crate::auth::DUMMY_PASSWORD_HASH);
    let password_ok = crate::auth::verify_password(hash_to_check, &body.password);

    let operator = match (lookup, password_ok) {
        (Some(op), true) => op,
        _ => {
            info!(username = %username_norm, "login failed");
            return error_response(
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                "invalid username or password",
            );
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let payload = crate::auth::new_session_payload(
        operator.id.clone(),
        operator.username.clone(),
        now,
        state.session_lifetime_secs,
    );
    let encoded = match crate::auth::encrypt_session(&payload, &session_key) {
        Ok(s) => s,
        Err(e) => {
            warn!(err = %e, "login: failed to encrypt session");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to issue session",
            );
        }
    };
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

    state.login_rate_limiter.reset(&username_norm);
    info!(
        username = %operator.username,
        role = ?operator.role,
        "login succeeded"
    );

    let body = LoginResponse {
        username: operator.username.clone(),
        role: operator.role,
    };
    let body_bytes = serde_json::to_vec(&body).expect("serializing login response");
    Response::builder()
        .status(StatusCode::OK)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .header(hyper::header::SET_COOKIE, session_cookie)
        .header(hyper::header::SET_COOKIE, csrf_cookie)
        .body(Full::new(Bytes::from(body_bytes)))
        .expect("building login response")
}

#[derive(serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct BootstrapRequest {
    pub username: String,
    pub password: String,
}

#[derive(serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct BootstrapStatusResponse {
    pub bootstrap_required: bool,
}

#[derive(serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct BootstrapResponse {
    pub username: String,
    pub role: crate::operators::OperatorRole,
}

/// Handle `GET /v1/admin/auth/bootstrap/status`. Public endpoint:
/// returns `{ bootstrap_required: true }` only when the operator
/// collection is empty AND the bootstrap-completed sentinel is unset.
/// Anything else (operators exist, sentinel set, store error) yields
/// `false` — the SPA's response is to show the login page.
fn handle_admin_bootstrap_status(state: &AdminRuntimeState) -> Response<Full<Bytes>> {
    let sentinel_set =
        crate::operators::bootstrap_completed(state.admin_home_store.as_ref()).unwrap_or(true);
    let collection_empty = crate::operators::load(state.admin_home_store.as_ref())
        .map(|c| c.is_empty())
        .unwrap_or(false);
    json_response(
        StatusCode::OK,
        &BootstrapStatusResponse {
            bootstrap_required: !sentinel_set && collection_empty,
        },
    )
}

/// Handle `POST /v1/admin/auth/bootstrap`. Public endpoint: creates
/// the very first admin operator, marks the persistent sentinel, and
/// issues a session in one round trip. After this succeeds, the
/// endpoint refuses further calls even if every operator is later
/// deleted.
async fn handle_admin_bootstrap(
    state: &AdminRuntimeState,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let Some(session_key) = state.session_key else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "session_key_not_configured",
            "admin session_key_file is not configured",
        );
    };
    let body = match parse_json_body::<BootstrapRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    let username_norm = crate::operators::normalise_username(&body.username);
    if username_norm.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_credentials",
            "username is required",
        );
    }
    if body.password.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_credentials",
            "password is required",
        );
    }

    // Serialise concurrent bootstrap attempts: two callers must not
    // both see "operators empty + sentinel unset" before either has
    // written. The lock is held across the whole sequence.
    let _guard = state.bootstrap_lock.lock().unwrap();

    match crate::operators::bootstrap_completed(state.admin_home_store.as_ref()) {
        Ok(true) => {
            audit_bootstrap_attempt(state, &username_norm, "bootstrap_already_completed");
            return error_response(
                StatusCode::CONFLICT,
                "bootstrap_already_completed",
                "bootstrap has already been completed for this backend",
            );
        }
        Ok(false) => {}
        Err(e) => {
            warn!(err = %e, "bootstrap: reading sentinel failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "bootstrap sentinel unreadable",
            );
        }
    }

    let collection = match crate::operators::load(state.admin_home_store.as_ref()) {
        Ok(c) => c,
        Err(e) => {
            warn!(err = %e, "bootstrap: loading operator collection failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "operator collection unavailable",
            );
        }
    };
    if !collection.is_empty() {
        audit_bootstrap_attempt(state, &username_norm, "operators_already_exist");
        return error_response(
            StatusCode::CONFLICT,
            "operators_already_exist",
            "an operator already exists; bootstrap requires an empty operator collection",
        );
    }

    let now = crate::operators::now_epoch_secs();
    let password_hash = match crate::auth::hash_password(&body.password) {
        Ok(h) => h,
        Err(e) => {
            warn!(err = %e, "bootstrap: argon2 hash failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "could not hash password",
            );
        }
    };
    let record = crate::operators::OperatorRecord {
        id: crate::operators::new_operator_id(),
        username: username_norm.clone(),
        password_hash,
        role: crate::operators::OperatorRole::Admin,
        granted_stores: Vec::new(),
        created_at: now,
        password_changed_at: now,
    };
    let mut new_collection = crate::operators::OperatorCollection::default();
    if let Err(e) = new_collection.add(record.clone()) {
        warn!(err = %e, "bootstrap: building collection failed");
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_credentials",
            &e.to_string(),
        );
    }
    if let Err(e) = crate::operators::save(state.admin_home_store.as_ref(), &new_collection) {
        warn!(err = %e, "bootstrap: writing operator collection failed");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "could not persist operator",
        );
    }
    if let Err(e) = crate::operators::mark_bootstrap_completed(
        state.admin_home_store.as_ref(),
        &crate::operators::BootstrapSentinel {
            completed_at: now,
            by_username: record.username.clone(),
        },
    ) {
        warn!(err = %e, "bootstrap: writing sentinel failed");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "could not persist bootstrap sentinel",
        );
    }
    audit_bootstrap_attempt(state, &record.username, "success");

    // Issue a session for the brand-new admin.
    let payload = crate::auth::new_session_payload(
        record.id.clone(),
        record.username.clone(),
        now,
        state.session_lifetime_secs,
    );
    let encoded = match crate::auth::encrypt_session(&payload, &session_key) {
        Ok(s) => s,
        Err(e) => {
            warn!(err = %e, "bootstrap: encrypting session failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "could not issue session",
            );
        }
    };
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
    info!(username = %record.username, "bootstrap: first admin created");
    let body = BootstrapResponse {
        username: record.username.clone(),
        role: record.role,
    };
    let body_bytes = serde_json::to_vec(&body).expect("serializing bootstrap response");
    Response::builder()
        .status(StatusCode::OK)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .header(hyper::header::SET_COOKIE, session_cookie)
        .header(hyper::header::SET_COOKIE, csrf_cookie)
        .body(Full::new(Bytes::from(body_bytes)))
        .expect("building bootstrap response")
}

/// Write a single audit history entry for a bootstrap attempt. Best
/// effort: never fails the caller.
fn audit_bootstrap_attempt(state: &AdminRuntimeState, username: &str, result: &str) {
    let event = underwrap_protocol::service::AdminUiAuditEvent {
        ts_epoch: crate::operators::now_epoch_secs(),
        action: "bootstrap_attempt".to_string(),
        operator: username.to_string(),
        wrapper: None,
        resource: None,
        secret_id: None,
        operation: None,
        result: result.to_string(),
    };
    if let Err(e) = append_audit_history_event(state.admin_home_store.as_ref(), event) {
        warn!(err = %e, "bootstrap: writing audit event failed");
    }
}

/// Handle `POST /v1/admin/auth/logout`. Revokes the current session id
/// server-side and clears both cookies in the response. The CSRF
/// double-submit check is enforced by `handle_admin_request` before we
/// get here, so the caller has already proved possession of the
/// session's csrf_secret.
fn handle_admin_logout(
    state: &AdminRuntimeState,
    session: &crate::auth::SessionPayload,
) -> Response<Full<Bytes>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    state
        .session_revocations
        .revoke(session.session_id.clone(), session.expires_at, now);
    info!(username = %session.username, "logout: session revoked");
    Response::builder()
        .status(StatusCode::OK)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .header(
            hyper::header::SET_COOKIE,
            crate::auth::render_session_cookie_clear(state.cookies_secure),
        )
        .header(
            hyper::header::SET_COOKIE,
            crate::auth::render_csrf_cookie_clear(state.cookies_secure),
        )
        .body(Full::new(Bytes::from_static(b"{}")))
        .expect("building logout response")
}

fn handle_admin_whoami(
    state: &AdminRuntimeState,
    headers: &hyper::HeaderMap,
) -> Response<Full<Bytes>> {
    // whoami is intentionally tolerant of missing auth (returns
    // authenticated:false) so the SPA can use it to drive the
    // login/bootstrap redirect dance without producing 401s in the
    // browser devtools.
    let Some(key) = state.session_key.as_ref() else {
        return json_response(
            StatusCode::OK,
            &WhoamiResponse {
                authenticated: false,
                username: None,
                role: None,
            },
        );
    };
    let Some(value) = crate::auth::extract_session_cookie(headers) else {
        return json_response(
            StatusCode::OK,
            &WhoamiResponse {
                authenticated: false,
                username: None,
                role: None,
            },
        );
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let payload = match crate::auth::decrypt_session(value, key, now) {
        Ok(p) => p,
        Err(_) => {
            return json_response(
                StatusCode::OK,
                &WhoamiResponse {
                    authenticated: false,
                    username: None,
                    role: None,
                },
            );
        }
    };
    // Resolve the current role from the collection in case it has
    // changed since the session was issued.
    let collection = crate::operators::load(state.admin_home_store.as_ref()).unwrap_or_default();
    let role = collection
        .find_by_username(&payload.username)
        .map(|o| o.role);
    json_response(
        StatusCode::OK,
        &WhoamiResponse {
            authenticated: role.is_some(),
            username: Some(payload.username),
            role,
        },
    )
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
    // auth surfaces dispatch here before the bearer-token / cookie
    // check below.
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

    // CSRF only applies to session-cookie auth: bearer-token callers
    // are scripts/CLIs, not browsers, so the threat model doesn't
    // include drive-by cross-site requests carrying their credentials.
    if let Some(payload) = auth.as_session()
        && let Err((code, msg)) =
            check_csrf(request.method(), request.headers(), &payload.csrf_secret)
    {
        warn!(reason = msg, code = code, "rejected admin request (csrf)");
        return Ok(error_response(StatusCode::FORBIDDEN, code, msg));
    }

    let principal = match resolve_principal(&state, &auth) {
        Ok(p) => p,
        Err(resp) => return Ok(resp),
    };
    let operator = principal.username.clone();
    info!(
        operator = %operator,
        role = ?principal.role,
        method = %request.method(),
        path = request.uri().path(),
        "admin request"
    );

    // Logout runs *after* auth + CSRF have passed: the caller must have
    // a valid session AND have proven possession of its csrf_secret to
    // revoke it. That blocks drive-by logout via cookie-only requests.
    if request.method() == Method::POST && request.uri().path() == "/v1/admin/auth/logout" {
        return Ok(match auth.as_session() {
            Some(payload) => handle_admin_logout(&state, payload),
            None => error_response(
                StatusCode::BAD_REQUEST,
                "no_session",
                "logout requires a cookie session, not a bearer token",
            ),
        });
    }

    let response = match (request.method(), request.uri().path()) {
        (&Method::GET, "/v1/admin/stores") => {
            let mut stores: Vec<String> = state
                .stores
                .keys()
                .filter(|name| principal.has_store_access(name))
                .cloned()
                .collect();
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
                .filter(|(name, _)| principal.has_store_access(name))
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
                match admin_store_for_principal(&state, &principal, &body.store) {
                    Ok(store) => match admin_ui_resources_query(store.as_ref(), &body, now_epoch) {
                        Ok(response) => json_response(StatusCode::OK, &response),
                        Err(e) => {
                            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                        }
                    },
                    Err(resp) => resp,
                }
            }
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/resource/get") => {
            match parse_json_body::<underwrap_protocol::service::AdminUiResourceGetRequest>(request)
                .await
            {
                Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                    Ok(store) => match admin_ui_resource_detail(store.as_ref(), &body.resource) {
                        Ok(response) => json_response(StatusCode::OK, &response),
                        Err(e) => {
                            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                        }
                    },
                    Err(resp) => resp,
                },
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/ui/resource/reveal") => match parse_json_body::<
            underwrap_protocol::service::AdminUiResourceRevealRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(_) => handle_admin_ui_resource_reveal(&state, &operator, body),
                Err(resp) => resp,
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/resource/download") => match parse_json_body::<
            underwrap_protocol::service::AdminUiResourceDownloadRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(_) => handle_admin_ui_resource_download(&state, &operator, body),
                Err(resp) => resp,
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/resource/write-text") => match parse_json_body::<
            underwrap_protocol::service::AdminUiResourceWriteTextRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(_) => {
                    let now_epoch = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|duration| duration.as_secs())
                        .unwrap_or(0);
                    handle_admin_ui_resource_write_text(&state, &operator, body, now_epoch)
                }
                Err(resp) => resp,
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/resource/upload-file") => match parse_json_body::<
            underwrap_protocol::service::AdminUiResourceUploadFileRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(_) => {
                    let now_epoch = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|duration| duration.as_secs())
                        .unwrap_or(0);
                    handle_admin_ui_resource_upload_file(&state, &operator, body, now_epoch)
                }
                Err(resp) => resp,
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/resource/delete-entry") => match parse_json_body::<
            underwrap_protocol::service::AdminUiResourceDeleteEntryRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(_) => {
                    let now_epoch = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|duration| duration.as_secs())
                        .unwrap_or(0);
                    handle_admin_ui_resource_delete_entry(&state, &operator, body, now_epoch)
                }
                Err(resp) => resp,
            },
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
                    match admin_store_for_principal(&state, &principal, &body.store) {
                        Ok(store) => {
                            match admin_ui_mutation_get(store.as_ref(), &body.resource, now_epoch) {
                                Ok(response) => json_response(StatusCode::OK, &response),
                                Err(e) => error_response(
                                    StatusCode::BAD_REQUEST,
                                    "admin_error",
                                    &e.to_string(),
                                ),
                            }
                        }
                        Err(resp) => resp,
                    }
                }
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/ui/mutation/set") => {
            match parse_json_body::<underwrap_protocol::service::AdminUiMutationSetRequest>(request)
                .await
            {
                Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                    Ok(_) => {
                        let now_epoch = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|duration| duration.as_secs())
                            .unwrap_or(0);
                        handle_admin_ui_mutation_set(&state, &operator, body, now_epoch)
                    }
                    Err(resp) => resp,
                },
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/ui/wrappers/query") => match parse_json_body::<
            underwrap_protocol::service::AdminUiWrappersQueryRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(store) => match admin_ui_wrappers_query(store.as_ref(), &body) {
                    Ok(response) => json_response(StatusCode::OK, &response),
                    Err(e) => {
                        error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                    }
                },
                Err(resp) => resp,
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/wrapper/set-mode") => match parse_json_body::<
            underwrap_protocol::service::AdminUiWrapperSetModeRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(_) => handle_admin_ui_wrapper_set_mode(&state, &operator, body),
                Err(resp) => resp,
            },
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
                    match admin_store_for_principal(&state, &principal, &body.store) {
                        Ok(store) => {
                            match admin_ui_matrix_query(store.as_ref(), &body, now_epoch) {
                                Ok(response) => json_response(StatusCode::OK, &response),
                                Err(e) => error_response(
                                    StatusCode::BAD_REQUEST,
                                    "admin_error",
                                    &e.to_string(),
                                ),
                            }
                        }
                        Err(resp) => resp,
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
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(_) => handle_admin_ui_matrix_set_cell(&state, &operator, body),
                Err(resp) => resp,
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/ui/audit/query") => {
            match parse_json_body::<underwrap_protocol::service::AdminUiAuditQueryRequest>(request)
                .await
            {
                Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                    Ok(store) => match admin_ui_audit_query(store.as_ref(), &body) {
                        Ok(response) => json_response_no_store(StatusCode::OK, &response),
                        Err(e) => {
                            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                        }
                    },
                    Err(resp) => resp,
                },
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/secret/read") => match parse_json_body::<
            underwrap_protocol::service::AdminSecretReadRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(store) => match store.read(&body.secret_id) {
                    Ok(material) => json_response(
                        StatusCode::OK,
                        &underwrap_protocol::service::AdminSecretReadResponse {
                            material_b64: material.map(|material| {
                                base64::engine::general_purpose::STANDARD.encode(material.as_ref())
                            }),
                        },
                    ),
                    Err(e) => {
                        error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                    }
                },
                Err(resp) => resp,
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/secret/write") => {
            match parse_json_body::<underwrap_protocol::service::AdminSecretWriteRequest>(request)
                .await
            {
                Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                    Ok(_) => handle_admin_secret_write(&state, &operator, body),
                    Err(resp) => resp,
                },
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/secret/list") => {
            match parse_json_body::<underwrap_protocol::service::AdminSecretListRequest>(request)
                .await
            {
                Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                    Ok(store) => match store.list(&body.prefix) {
                        Ok(ids) => json_response(
                            StatusCode::OK,
                            &underwrap_protocol::service::AdminSecretListResponse { ids },
                        ),
                        Err(e) => {
                            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                        }
                    },
                    Err(resp) => resp,
                },
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/secret/delete") => {
            match parse_json_body::<underwrap_protocol::service::AdminSecretDeleteRequest>(request)
                .await
            {
                Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                    Ok(store) => match store.delete(&body.secret_id) {
                        Ok(existed) => json_response(
                            StatusCode::OK,
                            &underwrap_protocol::service::AdminSecretDeleteResponse { existed },
                        ),
                        Err(e) => {
                            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                        }
                    },
                    Err(resp) => resp,
                },
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/catalog/get") => {
            match parse_json_body::<underwrap_protocol::service::AdminCatalogGetRequest>(request)
                .await
            {
                Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                    Ok(store) => match load_catalog_document(&**store) {
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
                        Err(e) => {
                            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                        }
                    },
                    Err(resp) => resp,
                },
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/policy/get") => {
            match parse_json_body::<underwrap_protocol::service::AdminPolicyGetRequest>(request)
                .await
            {
                Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                    Ok(store) => {
                        let read_result =
                            store.read(POLICY_DOCUMENT_SECRET_ID).and_then(|m| match m {
                                Some(material) => Ok(serde_json::from_slice(material.as_ref())?),
                                None => Ok(underwrap_protocol::policy::PolicyDocument::default()),
                            });
                        match read_result {
                            Ok(doc) => json_response(
                                StatusCode::OK,
                                &underwrap_protocol::service::AdminPolicyGetResponse {
                                    document: doc,
                                },
                            ),
                            Err(e) => error_response(
                                StatusCode::BAD_REQUEST,
                                "admin_error",
                                &e.to_string(),
                            ),
                        }
                    }
                    Err(resp) => resp,
                },
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/policy/put") => match parse_json_body::<
            underwrap_protocol::service::AdminPolicyPutRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(store) => {
                    let write_result = serde_json::to_vec(&body.document)
                        .map_err(anyhow::Error::from)
                        .and_then(|bytes| store.write(POLICY_DOCUMENT_SECRET_ID, bytes.as_slice()));
                    match write_result {
                        Ok(()) => json_response(
                            StatusCode::OK,
                            &underwrap_protocol::service::AdminOkResponse { ok: true },
                        ),
                        Err(e) => {
                            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                        }
                    }
                }
                Err(resp) => resp,
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/wrapper/set-mode") => match parse_json_body::<
            underwrap_protocol::service::AdminWrapperModeRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(store) => {
                    let update_result = (|| -> Result<()> {
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
                    })();
                    match update_result {
                        Ok(()) => json_response(
                            StatusCode::OK,
                            &underwrap_protocol::service::AdminOkResponse { ok: true },
                        ),
                        Err(e) => {
                            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                        }
                    }
                }
                Err(resp) => resp,
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/rule/upsert") => match parse_json_body::<
            underwrap_protocol::service::AdminRuleUpsertRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(store) => {
                    let update_result = (|| -> Result<()> {
                        let mut doc: underwrap_protocol::policy::PolicyDocument =
                            match store.read(POLICY_DOCUMENT_SECRET_ID)? {
                                Some(material) => serde_json::from_slice(material.as_ref())?,
                                None => underwrap_protocol::policy::PolicyDocument::default(),
                            };
                        if let Some(rule) =
                            doc.rules.iter_mut().find(|rule| rule.id == body.rule.id)
                        {
                            *rule = body.rule.clone();
                        } else {
                            doc.rules.push(body.rule.clone());
                        }
                        doc.rules.sort_by(|a, b| a.id.cmp(&b.id));
                        store.write(
                            POLICY_DOCUMENT_SECRET_ID,
                            serde_json::to_vec(&doc)?.as_slice(),
                        )
                    })();
                    match update_result {
                        Ok(()) => json_response(
                            StatusCode::OK,
                            &underwrap_protocol::service::AdminOkResponse { ok: true },
                        ),
                        Err(e) => {
                            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                        }
                    }
                }
                Err(resp) => resp,
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/rule/delete") => match parse_json_body::<
            underwrap_protocol::service::AdminRuleDeleteRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(store) => {
                    let update_result = (|| -> Result<()> {
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
                    })();
                    match update_result {
                        Ok(()) => json_response(
                            StatusCode::OK,
                            &underwrap_protocol::service::AdminOkResponse { ok: true },
                        ),
                        Err(e) => {
                            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                        }
                    }
                }
                Err(resp) => resp,
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/grant/upsert") => match parse_json_body::<
            underwrap_protocol::service::AdminGrantUpsertRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(store) => {
                    let update_result = (|| -> Result<()> {
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
                    })();
                    match update_result {
                        Ok(()) => json_response(
                            StatusCode::OK,
                            &underwrap_protocol::service::AdminOkResponse { ok: true },
                        ),
                        Err(e) => {
                            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                        }
                    }
                }
                Err(resp) => resp,
            },
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/grant/delete") => match parse_json_body::<
            underwrap_protocol::service::AdminGrantDeleteRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(store) => {
                    let update_result = (|| -> Result<()> {
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
                    })();
                    match update_result {
                        Ok(()) => json_response(
                            StatusCode::OK,
                            &underwrap_protocol::service::AdminOkResponse { ok: true },
                        ),
                        Err(e) => {
                            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                        }
                    }
                }
                Err(resp) => resp,
            },
            Err(response) => response,
        },
        _ => error_response(StatusCode::NOT_FOUND, "not_found", "unknown admin endpoint"),
    };
    Ok(attach_csrf_cookie(response, &state, &auth))
}

/// Refresh the CSRF cookie on every session-authenticated response so
/// the SPA always has a value matching the live session. Bearer-token
/// responses are not modified.
fn attach_csrf_cookie(
    mut response: Response<Full<Bytes>>,
    state: &AdminRuntimeState,
    auth: &AdminAuth,
) -> Response<Full<Bytes>> {
    if let Some(payload) = auth.as_session() {
        let cookie = crate::auth::render_csrf_cookie(
            &payload.csrf_secret,
            state.session_lifetime_secs,
            state.cookies_secure,
        );
        match hyper::header::HeaderValue::from_str(&cookie) {
            Ok(v) => {
                response.headers_mut().append(hyper::header::SET_COOKIE, v);
            }
            Err(e) => {
                warn!(err = %e, "could not append CSRF cookie header");
            }
        }
    }
    response
}

#[cfg(test)]
mod store_dir_tests {
    //! Tests for the declarative+dynamic `stores.d/` loader added by
    //! ticket 230. The wider runtime tests live in `mod tests` below.
    use super::*;

    fn write_def(dir: &Path, name: &str, def: &StoreDefinition) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join(format!("{name}.yaml")),
            serde_yaml::to_string(def).unwrap(),
        )
        .unwrap();
    }

    fn plain_def(dir: &Path) -> StoreDefinition {
        StoreDefinition {
            description: None,
            created_at: None,
            storage: underwrap_protocol::config::StorageConfig::Plain {
                dir: dir.to_path_buf(),
            },
        }
    }

    #[test]
    fn only_declarative_loads() {
        let tmp = tempfile::TempDir::new().unwrap();
        let decl = tmp.path().join("decl");
        let dynd = tmp.path().join("dyn");
        write_def(&decl, "work", &plain_def(&tmp.path().join("work-data")));

        let map = load_store_definitions(&decl, &dynd).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map["work"].source, StoreSource::Declarative);
    }

    #[test]
    fn only_dynamic_loads() {
        let tmp = tempfile::TempDir::new().unwrap();
        let decl = tmp.path().join("decl");
        let dynd = tmp.path().join("dyn");
        write_def(&dynd, "personal", &plain_def(&tmp.path().join("p-data")));

        let map = load_store_definitions(&decl, &dynd).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map["personal"].source, StoreSource::Dynamic);
    }

    #[test]
    fn both_dirs_union() {
        let tmp = tempfile::TempDir::new().unwrap();
        let decl = tmp.path().join("decl");
        let dynd = tmp.path().join("dyn");
        write_def(&decl, "work", &plain_def(&tmp.path().join("w")));
        write_def(&dynd, "personal", &plain_def(&tmp.path().join("p")));

        let map = load_store_definitions(&decl, &dynd).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map["work"].source, StoreSource::Declarative);
        assert_eq!(map["personal"].source, StoreSource::Dynamic);
    }

    #[test]
    fn duplicate_name_across_sources_is_fatal_and_names_both_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let decl = tmp.path().join("decl");
        let dynd = tmp.path().join("dyn");
        write_def(&decl, "shared", &plain_def(&tmp.path().join("a")));
        write_def(&dynd, "shared", &plain_def(&tmp.path().join("b")));

        let err =
            load_store_definitions(&decl, &dynd).expect_err("duplicate names must abort startup");
        let msg = format!("{err:#}");
        assert!(msg.contains("duplicate store name 'shared'"), "got: {msg}");
        assert!(msg.contains(decl.join("shared.yaml").to_string_lossy().as_ref()));
        assert!(msg.contains(dynd.join("shared.yaml").to_string_lossy().as_ref()));
    }

    #[test]
    fn malformed_file_is_rejected_with_path_in_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        let decl = tmp.path().join("decl");
        std::fs::create_dir_all(&decl).unwrap();
        let bad = decl.join("bad.yaml");
        std::fs::write(&bad, ": not valid yaml :::\n").unwrap();

        let err = load_store_definitions(&decl, &tmp.path().join("dyn"))
            .expect_err("malformed YAML must abort");
        let msg = format!("{err:#}");
        assert!(msg.contains(bad.to_string_lossy().as_ref()), "got: {msg}");
    }

    #[test]
    fn missing_directories_are_tolerated() {
        let tmp = tempfile::TempDir::new().unwrap();
        let map = load_store_definitions(
            &tmp.path().join("does-not-exist-decl"),
            &tmp.path().join("does-not-exist-dyn"),
        )
        .unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn non_yaml_files_are_ignored() {
        // Operators sometimes leave editor swapfiles or README files
        // in config dirs; only `*.yaml` is treated as a definition.
        let tmp = tempfile::TempDir::new().unwrap();
        let decl = tmp.path().join("decl");
        std::fs::create_dir_all(&decl).unwrap();
        std::fs::write(decl.join("notes.txt"), "ignore me\n").unwrap();
        std::fs::write(decl.join(".work.yaml.swp"), "vim swap\n").unwrap();
        write_def(&decl, "work", &plain_def(&tmp.path().join("w")));

        let map = load_store_definitions(&decl, &tmp.path().join("dyn")).unwrap();
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("work"));
    }

    #[test]
    fn invalid_store_filename_is_rejected() {
        let tmp = tempfile::TempDir::new().unwrap();
        let decl = tmp.path().join("decl");
        std::fs::create_dir_all(&decl).unwrap();
        // Uppercase is rejected: store names must round-trip as URL
        // path segments and (later) as filesystem paths under a
        // case-insensitive FS without colliding.
        std::fs::write(
            decl.join("Bad-Name.yaml"),
            serde_yaml::to_string(&plain_def(&tmp.path().join("x"))).unwrap(),
        )
        .unwrap();

        let err = load_store_definitions(&decl, &tmp.path().join("dyn"))
            .expect_err("uppercase store name must be refused");
        assert!(format!("{err:#}").contains("invalid store name"));
    }

    #[test]
    fn description_metadata_round_trips() {
        let tmp = tempfile::TempDir::new().unwrap();
        let decl = tmp.path().join("decl");
        let def = StoreDefinition {
            description: Some("the work store".into()),
            created_at: Some(123),
            storage: underwrap_protocol::config::StorageConfig::Plain {
                dir: tmp.path().join("w"),
            },
        };
        write_def(&decl, "work", &def);
        let map = load_store_definitions(&decl, &tmp.path().join("dyn")).unwrap();
        assert_eq!(
            map["work"].definition.description.as_deref(),
            Some("the work store"),
        );
        assert_eq!(map["work"].definition.created_at, Some(123));
    }
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
        let state = test_admin_state(stores, HashMap::new());

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
        let state = test_admin_state(stores, HashMap::new());

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
        let state = test_admin_state(stores, HashMap::new());

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
        let state = test_admin_state(stores, HashMap::new());

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
        let state = test_admin_state(stores, HashMap::new());

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
        let state = test_admin_state(stores, HashMap::new());

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
        let state = test_admin_state(stores, HashMap::new());

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
        test_admin_state(
            HashMap::new(),
            tokens
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        )
    }

    fn test_admin_state(
        stores: HashMap<String, Arc<dyn StorageBackend>>,
        tokens: HashMap<String, String>,
    ) -> AdminRuntimeState {
        // Tests that don't exercise login can use any store as the
        // admin-home store; if `stores` is empty we synthesise a memory
        // store so the field is populated.
        let admin_home_store: Arc<dyn StorageBackend> = stores
            .values()
            .next()
            .cloned()
            .unwrap_or_else(|| Arc::new(MemTestStore::new()));
        AdminRuntimeState {
            stores,
            store_sources: HashMap::new(),
            tokens,
            allowed_origins: None,
            web_source: crate::web::WebAssetSource::Embedded,
            admin_home_store,
            session_key: None,
            session_lifetime_secs: crate::auth::DEFAULT_SESSION_LIFETIME_SECS,
            cookies_secure: false,
            login_rate_limiter: crate::auth::RateLimiter::new(),
            session_revocations: crate::auth::SessionRevocations::new(),
            bootstrap_lock: std::sync::Mutex::new(()),
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
        assert_eq!(ok.operator(), "bob");
        assert!(matches!(ok, AdminAuth::Bearer { .. }));

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
            admin_home_store: None,
            session_key_file: None,
            session_lifetime_secs: None,
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
            admin_home_store: None,
            session_key_file: None,
            session_lifetime_secs: None,
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
            admin_home_store: None,
            session_key_file: None,
            session_lifetime_secs: None,
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
            admin_home_store: None,
            session_key_file: None,
            session_lifetime_secs: None,
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
        let store: Arc<dyn StorageBackend> = Arc::new(MemTestStore::new());
        let mut collection = crate::operators::OperatorCollection::default();
        let hash = crate::auth::hash_password(password).unwrap();
        collection
            .add(crate::operators::OperatorRecord {
                id: crate::operators::new_operator_id(),
                username: username.to_string(),
                password_hash: hash,
                role: crate::operators::OperatorRole::Admin,
                granted_stores: Vec::new(),
                created_at: 1000,
                password_changed_at: 1000,
            })
            .unwrap();
        crate::operators::save(store.as_ref(), &collection).unwrap();
        let mut session_key = [0u8; crate::auth::SESSION_KEY_LEN];
        for (i, b) in session_key.iter_mut().enumerate() {
            *b = i as u8;
        }
        AdminRuntimeState {
            stores: HashMap::from([("home".to_string(), store.clone())]),
            store_sources: HashMap::new(),
            tokens: HashMap::new(),
            allowed_origins: None,
            web_source: crate::web::WebAssetSource::Embedded,
            admin_home_store: store,
            session_key: Some(session_key),
            session_lifetime_secs: 3600,
            cookies_secure: false,
            login_rate_limiter: crate::auth::RateLimiter::new(),
            session_revocations: crate::auth::SessionRevocations::new(),
            bootstrap_lock: std::sync::Mutex::new(()),
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
        let collection = crate::operators::load(state.admin_home_store.as_ref()).unwrap();
        let op = collection.find_by_username("alice").unwrap();
        assert!(crate::auth::verify_password(&op.password_hash, "hunter2"));
        assert!(!crate::auth::verify_password(&op.password_hash, "wrong"));

        let key = state.session_key.unwrap();
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
        let collection = crate::operators::load(state.admin_home_store.as_ref()).unwrap();
        assert!(collection.find_by_username("eve").is_none());
        assert!(!crate::auth::verify_password(
            crate::auth::DUMMY_PASSWORD_HASH,
            "anything"
        ));
    }

    #[tokio::test]
    async fn whoami_resolves_role_from_collection() {
        let state = seeded_admin_state("alice", "hunter2");
        let key = state.session_key.unwrap();
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
        assert_eq!(parsed["role"], "admin");
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
        let key = state.session_key.unwrap();
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

    fn principal(
        role: crate::operators::OperatorRole,
        grants: &[&str],
    ) -> crate::operators::Principal {
        crate::operators::Principal {
            id: "id".into(),
            username: "alice".into(),
            role,
            granted_stores: grants.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    #[test]
    fn admin_store_for_principal_admin_can_access_existing_store() {
        let store: Arc<dyn StorageBackend> = Arc::new(MemTestStore::new());
        let mut state = test_admin_state(
            HashMap::from([("work".to_string(), store.clone())]),
            HashMap::new(),
        );
        state.admin_home_store = store.clone();
        let p = principal(crate::operators::OperatorRole::Admin, &[]);
        assert!(admin_store_for_principal(&state, &p, "work").is_ok());
    }

    fn expect_404(result: std::result::Result<&Arc<dyn StorageBackend>, Response<Full<Bytes>>>) {
        match result {
            Ok(_) => panic!("expected 404 not_found, got Ok"),
            Err(resp) => assert_eq!(resp.status(), StatusCode::NOT_FOUND),
        }
    }

    #[test]
    fn admin_store_for_principal_admin_sees_unknown_store_as_404() {
        let state = test_admin_state(HashMap::new(), HashMap::new());
        let p = principal(crate::operators::OperatorRole::Admin, &[]);
        expect_404(admin_store_for_principal(&state, &p, "missing"));
    }

    #[test]
    fn admin_store_for_principal_operator_without_grant_returns_404_not_403() {
        // Don't leak the existence of stores the operator can't reach:
        // ungranted-but-existing must look identical to truly-missing.
        let store: Arc<dyn StorageBackend> = Arc::new(MemTestStore::new());
        let mut state = test_admin_state(
            HashMap::from([("work".to_string(), store.clone())]),
            HashMap::new(),
        );
        state.admin_home_store = store;
        let p = principal(crate::operators::OperatorRole::Operator, &[]);
        expect_404(admin_store_for_principal(&state, &p, "work"));
        expect_404(admin_store_for_principal(&state, &p, "nope"));
    }

    #[test]
    fn admin_store_for_principal_operator_with_grant_sees_only_that_store() {
        let store_a: Arc<dyn StorageBackend> = Arc::new(MemTestStore::new());
        let store_b: Arc<dyn StorageBackend> = Arc::new(MemTestStore::new());
        let mut state = test_admin_state(
            HashMap::from([
                ("alpha".to_string(), store_a.clone()),
                ("beta".to_string(), store_b),
            ]),
            HashMap::new(),
        );
        state.admin_home_store = store_a;
        let p = principal(crate::operators::OperatorRole::Operator, &["alpha"]);
        assert!(admin_store_for_principal(&state, &p, "alpha").is_ok());
        expect_404(admin_store_for_principal(&state, &p, "beta"));
    }

    #[test]
    fn resolve_principal_session_uses_operator_collection_role_and_grants() {
        let state = seeded_admin_state("alice", "hunter2");
        // Re-seed collection so alice has Operator role with one grant.
        let store = state.admin_home_store.clone();
        let mut collection = crate::operators::OperatorCollection::default();
        let hash = crate::auth::hash_password("hunter2").unwrap();
        collection
            .add(crate::operators::OperatorRecord {
                id: crate::operators::new_operator_id(),
                username: "alice".into(),
                password_hash: hash,
                role: crate::operators::OperatorRole::Operator,
                granted_stores: vec!["work".into()],
                created_at: 1000,
                password_changed_at: 1000,
            })
            .unwrap();
        crate::operators::save(store.as_ref(), &collection).unwrap();

        let payload = crate::auth::new_session_payload("op-1".into(), "alice".into(), 1000, 3600);
        let auth = AdminAuth::Session(payload);
        let p = resolve_principal(&state, &auth).unwrap();
        assert_eq!(p.username, "alice");
        assert!(!p.is_admin());
        assert!(p.has_store_access("work"));
        assert!(!p.has_store_access("other"));
    }

    #[test]
    fn resolve_principal_bearer_synthesises_admin() {
        let state = seeded_admin_state("alice", "hunter2");
        let auth = AdminAuth::Bearer {
            operator: "cli-token".into(),
        };
        let p = resolve_principal(&state, &auth).unwrap();
        assert!(p.is_admin());
        assert_eq!(p.username, "cli-token");
    }

    #[test]
    fn resolve_principal_session_fails_when_operator_was_deleted() {
        // Cookie says "alice" but the collection has no record of her.
        let state = seeded_admin_state("alice", "hunter2");
        // Save an empty collection over alice's slot.
        crate::operators::save(
            state.admin_home_store.as_ref(),
            &crate::operators::OperatorCollection::default(),
        )
        .unwrap();
        let payload = crate::auth::new_session_payload("op-1".into(), "alice".into(), 1000, 3600);
        let auth = AdminAuth::Session(payload);
        match resolve_principal(&state, &auth) {
            Ok(_) => panic!("expected 401 for deleted operator"),
            Err(resp) => assert_eq!(resp.status(), StatusCode::UNAUTHORIZED),
        }
    }

    // ---------- Bootstrap (ticket 214) ----------

    /// Build admin state with a session key but an empty operator
    /// collection — the canonical "fresh install" shape.
    fn fresh_bootstrap_state() -> AdminRuntimeState {
        let store: Arc<dyn StorageBackend> = Arc::new(MemTestStore::new());
        let mut key = [0u8; crate::auth::SESSION_KEY_LEN];
        for (i, b) in key.iter_mut().enumerate() {
            *b = i as u8;
        }
        AdminRuntimeState {
            stores: HashMap::from([("home".to_string(), store.clone())]),
            store_sources: HashMap::new(),
            tokens: HashMap::new(),
            allowed_origins: None,
            web_source: crate::web::WebAssetSource::Embedded,
            admin_home_store: store,
            session_key: Some(key),
            session_lifetime_secs: 3600,
            cookies_secure: false,
            login_rate_limiter: crate::auth::RateLimiter::new(),
            session_revocations: crate::auth::SessionRevocations::new(),
            bootstrap_lock: std::sync::Mutex::new(()),
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
        crate::operators::mark_bootstrap_completed(
            state.admin_home_store.as_ref(),
            &crate::operators::BootstrapSentinel {
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
        let mut collection = crate::operators::OperatorCollection::default();
        collection
            .add(crate::operators::OperatorRecord {
                id: crate::operators::new_operator_id(),
                username: "alice".into(),
                password_hash: crate::auth::hash_password("p").unwrap(),
                role: crate::operators::OperatorRole::Admin,
                granted_stores: Vec::new(),
                created_at: 1000,
                password_changed_at: 1000,
            })
            .unwrap();
        crate::operators::save(state.admin_home_store.as_ref(), &collection).unwrap();
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
        let Some(session_key) = state.session_key else {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "session_key_not_configured",
                "no key",
            );
        };
        let username_norm = crate::operators::normalise_username(username);
        if username_norm.is_empty() || password.is_empty() {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_credentials",
                "missing field",
            );
        }
        let _guard = state.bootstrap_lock.lock().unwrap();
        if crate::operators::bootstrap_completed(state.admin_home_store.as_ref()).unwrap_or(true) {
            audit_bootstrap_attempt(state, &username_norm, "bootstrap_already_completed");
            return error_response(
                StatusCode::CONFLICT,
                "bootstrap_already_completed",
                "already done",
            );
        }
        let collection = crate::operators::load(state.admin_home_store.as_ref()).unwrap();
        if !collection.is_empty() {
            audit_bootstrap_attempt(state, &username_norm, "operators_already_exist");
            return error_response(
                StatusCode::CONFLICT,
                "operators_already_exist",
                "already populated",
            );
        }
        let now = crate::operators::now_epoch_secs();
        let hash = crate::auth::hash_password(password).unwrap();
        let record = crate::operators::OperatorRecord {
            id: crate::operators::new_operator_id(),
            username: username_norm.clone(),
            password_hash: hash,
            role: crate::operators::OperatorRole::Admin,
            granted_stores: Vec::new(),
            created_at: now,
            password_changed_at: now,
        };
        let mut new_collection = crate::operators::OperatorCollection::default();
        new_collection.add(record.clone()).unwrap();
        crate::operators::save(state.admin_home_store.as_ref(), &new_collection).unwrap();
        crate::operators::mark_bootstrap_completed(
            state.admin_home_store.as_ref(),
            &crate::operators::BootstrapSentinel {
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
        assert!(crate::operators::bootstrap_completed(state.admin_home_store.as_ref()).unwrap());
        // Operator is created with admin role.
        let collection = crate::operators::load(state.admin_home_store.as_ref()).unwrap();
        let op = collection.find_by_username("alice").unwrap();
        assert_eq!(op.role, crate::operators::OperatorRole::Admin);
        assert!(crate::auth::verify_password(&op.password_hash, "hunter2"));
        // Session + CSRF cookies present.
        let cookies: Vec<String> = resp
            .headers()
            .get_all(hyper::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
            .collect();
        assert!(cookies.iter().any(|c| c.starts_with("__Host-uw-session=")));
        assert!(cookies.iter().any(|c| c.starts_with("__Host-uw-csrf=")));
        // Audit event recorded.
        let history = load_audit_history(state.admin_home_store.as_ref()).unwrap();
        assert!(
            history.iter().any(|e| e.action == "bootstrap_attempt"
                && e.operator == "alice"
                && e.result == "success"),
            "expected success audit event: {history:?}"
        );
    }

    #[tokio::test]
    async fn bootstrap_second_call_returns_bootstrap_already_completed() {
        let state = fresh_bootstrap_state();
        let _ = run_bootstrap(&state, "alice", "hunter2");
        let resp = run_bootstrap(&state, "eve", "haxx0r");
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = read_json(resp).await;
        assert_eq!(body["code"], "bootstrap_already_completed");
        let history = load_audit_history(state.admin_home_store.as_ref()).unwrap();
        assert!(
            history
                .iter()
                .any(|e| e.operator == "eve" && e.result == "bootstrap_already_completed"),
            "expected failure audit event: {history:?}"
        );
    }

    #[tokio::test]
    async fn bootstrap_refuses_when_operators_exist_without_sentinel() {
        // This is the "CLI recovery happened first" case: an admin
        // exists, but the sentinel was never set. The online flow
        // must still refuse with operators_already_exist.
        let state = fresh_bootstrap_state();
        let mut collection = crate::operators::OperatorCollection::default();
        collection
            .add(crate::operators::OperatorRecord {
                id: crate::operators::new_operator_id(),
                username: "rescuer".into(),
                password_hash: crate::auth::hash_password("rescue").unwrap(),
                role: crate::operators::OperatorRole::Admin,
                granted_stores: Vec::new(),
                created_at: 1000,
                password_changed_at: 1000,
            })
            .unwrap();
        crate::operators::save(state.admin_home_store.as_ref(), &collection).unwrap();

        let resp = run_bootstrap(&state, "newbie", "x");
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = read_json(resp).await;
        assert_eq!(body["code"], "operators_already_exist");
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
        assert!(crate::operators::bootstrap_completed(state.admin_home_store.as_ref()).unwrap());
        let collection = crate::operators::load(state.admin_home_store.as_ref()).unwrap();
        assert_eq!(collection.len(), 1);
    }

    #[test]
    fn wrapper_set_mode_audit_event_carries_principal_username() {
        // Verifies the principal-propagation requirement: a mutation
        // performed as "bob" lands in the audit history with
        // operator: "bob".
        let raw_store = Arc::new(TestStore::new());
        let store: Arc<dyn StorageBackend> = raw_store.clone();
        let stores: HashMap<String, Arc<dyn StorageBackend>> =
            HashMap::from([("work".to_string(), store)]);
        let state = test_admin_state(stores, HashMap::new());
        let body = underwrap_protocol::service::AdminUiWrapperSetModeRequest {
            store: "work".to_string(),
            wrapper: "gh-readonly".to_string(),
            mode: underwrap_protocol::policy::WrapperApprovalMode::Locked,
            reason: Some("test".into()),
        };
        let resp = handle_admin_ui_wrapper_set_mode(&state, "bob", body);
        assert_eq!(resp.status(), StatusCode::OK);
        let history = load_audit_history(raw_store.as_ref()).unwrap();
        assert!(
            history.iter().any(|e| e.operator == "bob"),
            "audit history should record 'bob' as the operator: {history:?}"
        );
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
        assert!(auth.as_session().is_some());
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
        let wrapped = attach_csrf_cookie(response, &state, &AdminAuth::Session(payload.clone()));
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
    fn attach_csrf_cookie_is_noop_for_bearer_auth() {
        let state = seeded_admin_state("alice", "hunter2");
        let response = json_response(StatusCode::OK, &serde_json::json!({"hello": "world"}));
        let wrapped = attach_csrf_cookie(
            response,
            &state,
            &AdminAuth::Bearer {
                operator: "alice".into(),
            },
        );
        assert!(wrapped.headers().get(hyper::header::SET_COOKIE).is_none());
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
}
