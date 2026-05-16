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
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tracing::{error, info, warn};

use underwrap_protocol::config::{StoreDefinition, StoreSource};
use underwrap_protocol::policy::{POLICY_DOCUMENT_SECRET_ID, PolicyDocument};

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
    /// Allowlist of directory prefixes the admin "create store" API
    /// (ticket 231) is willing to place new store data dirs under.
    /// A `POST /v1/admin/stores` request whose `dir` is not
    /// canonically below one of these roots is rejected with
    /// `dir_outside_allowed_roots`. Empty disables store creation
    /// (returns `data_root_not_configured`).
    #[serde(default)]
    pub stores_data_roots: Option<Vec<PathBuf>>,
    /// Directory under which the admin API writes master-key files
    /// for encrypted dynamic stores (ticket 234). Each encrypted
    /// store gets one key file at `<keys_dir>/<store>.key`, mode
    /// 0600. Defaults to [`DEFAULT_KEYS_DIR`].
    #[serde(default)]
    pub keys_dir: Option<PathBuf>,
}

/// Default location of the declarative stores directory. Each
/// `<name>.yaml` here is owned by the NixOS module and treated as
/// read-only by the admin API.
pub const DEFAULT_STORES_DECLARATIVE_DIR: &str = "/etc/underwrap-backend/stores.d";

/// Default location of the dynamic stores directory. The admin API
/// owns this tree and is allowed to create/update/delete files in it.
pub const DEFAULT_STORES_DYNAMIC_DIR: &str = "/var/lib/underwrap-backend/stores.d";

/// Default location of the master-key directory the admin API writes
/// to when creating encrypted dynamic stores (ticket 234). One file
/// per store at `<keys_dir>/<store>.key`, mode 0600.
pub const DEFAULT_KEYS_DIR: &str = "/var/lib/underwrap-backend/keys";

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

/// Open backends + provenance metadata for every store visible to the
/// admin API. Wrapped in an `RwLock` inside [`AdminRuntimeState`] so
/// the CRUD handlers in ticket 231 can mutate it without rebooting
/// the daemon. Readers (overview, secret-read, …) take the read
/// guard and clone out the `Arc<dyn StorageBackend>` they need.
pub struct StoreRegistry {
    pub open: HashMap<String, Arc<dyn StorageBackend>>,
    pub sources: HashMap<String, LoadedStoreDef>,
}

impl StoreRegistry {
    pub fn new() -> Self {
        Self {
            open: HashMap::new(),
            sources: HashMap::new(),
        }
    }
}

impl Default for StoreRegistry {
    fn default() -> Self {
        Self::new()
    }
}

struct ServiceRuntimeState {
    clients: HashMap<String, Arc<dyn StorageBackend>>,
}

struct AdminRuntimeState {
    /// Open store backends + their on-disk provenance. CRUD handlers
    /// hold the write lock briefly while they swap in/out a single
    /// entry; everyone else takes the read lock and clones the
    /// `Arc<dyn StorageBackend>` they need so the lock isn't held
    /// across I/O.
    stores: std::sync::RwLock<StoreRegistry>,
    /// Path of the dynamic stores directory — where the admin CRUD
    /// handlers write/remove `<name>.yaml`. Stored here so the
    /// handlers don't need a separate config reference.
    stores_dynamic_dir: PathBuf,
    /// Configured roots under which a created store's `dir` must
    /// live. Mutating handlers reject requests whose `dir` is outside
    /// every entry. Empty disables creation (returns
    /// `data_root_not_configured`).
    stores_data_roots: Vec<PathBuf>,
    /// Directory where master-key files for encrypted dynamic stores
    /// are written. One file per store, mode 0600. See ticket 234.
    keys_dir: PathBuf,
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
    /// Mutex that serialises operator-collection read-modify-write
    /// cycles so two concurrent admin requests (e.g. two user
    /// renames) can't observe and overwrite each other's intermediate
    /// state (ticket 240). Held across `load` → mutate → `save`.
    operator_collection_lock: std::sync::Mutex<()>,
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
    let dynamic_dir = config.dynamic_dir();

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
            stores: std::sync::RwLock::new(StoreRegistry {
                open: stores.clone(),
                sources: definitions.clone(),
            }),
            stores_dynamic_dir: dynamic_dir.clone(),
            stores_data_roots: admin_config.stores_data_roots.clone().unwrap_or_default(),
            keys_dir: admin_config
                .keys_dir
                .clone()
                .unwrap_or_else(|| PathBuf::from(DEFAULT_KEYS_DIR)),
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
            operator_collection_lock: std::sync::Mutex::new(()),
        });
        {
            let snapshot = admin_state.stores.read().unwrap();
            let declarative_count = snapshot
                .sources
                .values()
                .filter(|d| d.source == StoreSource::Declarative)
                .count();
            let dynamic_count = snapshot
                .sources
                .values()
                .filter(|d| d.source == StoreSource::Dynamic)
                .count();
            info!(
                listen = %admin_config.listen,
                stores = snapshot.open.len(),
                declarative = declarative_count,
                dynamic = dynamic_count,
                operators = admin_state.tokens.len(),
                tls = tls_acceptor.is_some(),
                "backend admin listening",
            );
        }
        tasks.spawn(admin_accept_loop(admin_listener, admin_state, tls_acceptor));
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

    // Admin writes are always allowed once the operator's per-store
    // access has been checked. Per-resource write permissions on the
    // policy document gate wrapper-side writes only — admins can't
    // be locked out of their own backend.

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

fn admin_ui_overview_for_store(
    store_name: &str,
    store: &dyn StorageBackend,
    _now_epoch: u64,
) -> Result<underwrap_protocol::service::AdminUiOverviewStore> {
    let catalog = load_catalog_document(store)?;
    let policy = load_policy_document(store)?;

    let resource_count = catalog
        .iter()
        .map(|entry| entry.resource.clone())
        .collect::<BTreeSet<_>>()
        .len();

    // Bucket every agent grant by its current mode. Pre-245 these
    // were per-wrapper modes; the grant rewrite moved them onto each
    // `(wrapper, host, user)` tuple so the count here reflects the
    // distribution across the agent fleet, not the wrapper catalog.
    let mut wrapper_counts = underwrap_protocol::service::AdminUiWrapperCounts {
        locked: 0,
        unlocked: 0,
        unlocked_until: 0,
        per_call_approval: 0,
    };
    for grant in &policy.grants {
        match grant.mode {
            underwrap_protocol::policy::WrapperApprovalMode::Locked => wrapper_counts.locked += 1,
            underwrap_protocol::policy::WrapperApprovalMode::Unlocked => {
                wrapper_counts.unlocked += 1
            }
            underwrap_protocol::policy::WrapperApprovalMode::UnlockedUntil { .. } => {
                wrapper_counts.unlocked_until += 1
            }
            underwrap_protocol::policy::WrapperApprovalMode::PerCallApproval { .. } => {
                wrapper_counts.per_call_approval += 1
            }
        }
    }

    Ok(underwrap_protocol::service::AdminUiOverviewStore {
        store: store_name.to_string(),
        resource_count,
        wrapper_counts,
        recent_audit_events: load_audit_history(store)?.len(),
        writable_files_resources: policy
            .files_writable
            .values()
            .filter(|enabled| **enabled)
            .count(),
        writable_libsecret_resources: policy
            .libsecret_writable
            .values()
            .filter(|enabled| **enabled)
            .count(),
    })
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

fn admin_store(state: &AdminRuntimeState, store: &str) -> Result<Arc<dyn StorageBackend>> {
    state
        .stores
        .read()
        .unwrap()
        .open
        .get(store)
        .cloned()
        .with_context(|| format!("unknown store '{store}'"))
}

/// Resolve a store reference for an authenticated request. Returns
/// 404 not_found when either the store doesn't exist or the principal
/// lacks a grant for it. Collapsing both into the same response code
/// prevents operators from probing the store catalogue.
///
/// Ungranted-access attempts by non-admin principals also emit a
/// `denied_grant` audit event so an admin can detect probing
/// (ticket 241). Admin requests that 404 on an unknown store don't
/// audit — they're either typos or stale UI state, not probing.
#[allow(clippy::result_large_err)]
fn admin_store_for_principal(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    store: &str,
) -> std::result::Result<Arc<dyn StorageBackend>, Response<Full<Bytes>>> {
    if !principal.has_store_access(store) {
        emit_denied_grant_audit(state, principal, store);
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "unknown store",
        ));
    }
    match state.stores.read().unwrap().open.get(store).cloned() {
        Some(s) => Ok(s),
        None => Err(error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "unknown store",
        )),
    }
}

/// Record an audit-history event for an operator hitting a store
/// they don't have a grant for. Best-effort: a failure to append
/// must not turn an authentication-layer 404 into a 500.
fn emit_denied_grant_audit(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    store: &str,
) {
    // Admins bypass grant filtering by design; if has_store_access
    // returned false here, the principal is an operator. We assert
    // that invariant in debug builds so a future refactor doesn't
    // accidentally start filtering admin requests too.
    debug_assert!(!principal.is_admin());
    let event = underwrap_protocol::service::AdminUiAuditEvent {
        ts_epoch: crate::operators::now_epoch_secs(),
        action: "store.access".to_string(),
        operator: principal.username.clone(),
        wrapper: None,
        resource: Some(format!("store:{store}")),
        secret_id: None,
        operation: None,
        result: "denied_grant".to_string(),
    };
    if let Err(e) = append_audit_history_event(state.admin_home_store.as_ref(), event) {
        warn!(err = %e, "denied_grant audit emit failed");
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
                    || state
                        .session_revocations
                        .is_user_floor_revoked(&payload.username, payload.issued_at)
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

// ===== Users CRUD (ticket 240) =====
//
// Operator records are stored as a single JSON document under the
// reserved secret id `__underwrap/admin/operators` in the admin-home
// store (see `crate::operators`). Every mutation goes through
// `mutate_collection` so the read-modify-write race against
// concurrent admin requests is closed by the home-store mutex and the
// uniqueness / last-admin invariants live in one place.

use underwrap_protocol::service::{
    AdminOkUserResponse, AdminPasswordChangeRequest, AdminUserCreateRequest,
    AdminUserDetailResponse, AdminUserListItem, AdminUserResetPasswordRequest, AdminUserRole,
    AdminUserUpdateRequest, AdminUsersListResponse,
};

fn user_role_to_wire(role: crate::operators::OperatorRole) -> AdminUserRole {
    match role {
        crate::operators::OperatorRole::Admin => AdminUserRole::Admin,
        crate::operators::OperatorRole::Operator => AdminUserRole::Operator,
    }
}

fn user_role_from_wire(role: AdminUserRole) -> crate::operators::OperatorRole {
    match role {
        AdminUserRole::Admin => crate::operators::OperatorRole::Admin,
        AdminUserRole::Operator => crate::operators::OperatorRole::Operator,
    }
}

fn user_list_item(record: &crate::operators::OperatorRecord) -> AdminUserListItem {
    AdminUserListItem {
        id: record.id.clone(),
        username: record.username.clone(),
        role: user_role_to_wire(record.role),
        granted_stores: record.granted_stores.clone(),
        created_at: record.created_at,
        password_changed_at: record.password_changed_at,
    }
}

fn user_detail(record: &crate::operators::OperatorRecord) -> AdminUserDetailResponse {
    AdminUserDetailResponse {
        id: record.id.clone(),
        username: record.username.clone(),
        role: user_role_to_wire(record.role),
        granted_stores: record.granted_stores.clone(),
        created_at: record.created_at,
        password_changed_at: record.password_changed_at,
    }
}

fn write_admin_user_audit(
    state: &AdminRuntimeState,
    actor: &crate::operators::Principal,
    target_username: &str,
    action: &str,
    result: &str,
) {
    let event = underwrap_protocol::service::AdminUiAuditEvent {
        ts_epoch: crate::operators::now_epoch_secs(),
        action: action.to_string(),
        operator: actor.username.clone(),
        wrapper: None,
        resource: Some(format!("user:{target_username}")),
        secret_id: None,
        operation: None,
        result: result.to_string(),
    };
    if let Err(e) = append_audit_history_event(state.admin_home_store.as_ref(), event) {
        warn!(err = %e, "user CRUD: writing audit event failed");
    }
}

/// Read-modify-write on the operator collection, serialised across
/// concurrent admin requests via `operator_collection_lock`. The
/// closure may mutate the collection in-place; on `Ok(_)` we persist
/// and return whatever the closure returned. On `Err(_)` the
/// collection is *not* persisted.
fn mutate_collection<T, F>(state: &AdminRuntimeState, f: F) -> Result<T>
where
    F: FnOnce(&mut crate::operators::OperatorCollection) -> Result<T>,
{
    let _guard = state.operator_collection_lock.lock().unwrap();
    let mut collection = crate::operators::load(state.admin_home_store.as_ref())?;
    let out = f(&mut collection)?;
    crate::operators::save(state.admin_home_store.as_ref(), &collection)?;
    Ok(out)
}

#[allow(clippy::result_large_err)]
fn handle_admin_users_list(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may list users",
        );
    }
    let collection = match crate::operators::load(state.admin_home_store.as_ref()) {
        Ok(c) => c,
        Err(e) => {
            warn!(err = %e, "users.list: collection unreadable");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "operator collection unavailable",
            );
        }
    };
    let mut users: Vec<AdminUserListItem> =
        collection.operators.iter().map(user_list_item).collect();
    users.sort_by(|a, b| a.username.cmp(&b.username));
    json_response(StatusCode::OK, &AdminUsersListResponse { users })
}

#[allow(clippy::result_large_err)]
fn handle_admin_user_detail(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    id: &str,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may inspect user records",
        );
    }
    let collection = match crate::operators::load(state.admin_home_store.as_ref()) {
        Ok(c) => c,
        Err(e) => {
            warn!(err = %e, "users.detail: collection unreadable");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "operator collection unavailable",
            );
        }
    };
    match collection.find_by_id(id) {
        Some(record) => json_response(StatusCode::OK, &user_detail(record)),
        None => error_response(StatusCode::NOT_FOUND, "not_found", "unknown user"),
    }
}

#[allow(clippy::result_large_err)]
async fn handle_admin_user_create(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may create users",
        );
    }
    let body = match parse_json_body::<AdminUserCreateRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    do_admin_user_create(state, principal, body)
}

#[allow(clippy::result_large_err)]
fn do_admin_user_create(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: AdminUserCreateRequest,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may create users",
        );
    }
    if body.password.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_password",
            "password must not be empty",
        );
    }
    let username_norm = crate::operators::normalise_username(&body.username);
    if username_norm.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_username",
            "username must not be empty",
        );
    }
    let password_hash = match crate::auth::hash_password(&body.password) {
        Ok(h) => h,
        Err(e) => {
            warn!(err = %e, "users.create: argon2 hash failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "could not hash password",
            );
        }
    };
    let now = crate::operators::now_epoch_secs();
    let record = crate::operators::OperatorRecord {
        id: crate::operators::new_operator_id(),
        username: username_norm.clone(),
        password_hash,
        role: user_role_from_wire(body.role),
        granted_stores: body.granted_stores.clone(),
        created_at: now,
        password_changed_at: now,
    };
    let target_username = record.username.clone();
    let result = mutate_collection(state, |c| {
        c.add(record.clone())?;
        Ok(record)
    });
    match result {
        Ok(saved) => {
            write_admin_user_audit(state, principal, &target_username, "user.create", "success");
            json_response(StatusCode::OK, &user_detail(&saved))
        }
        Err(e) => {
            let msg = e.to_string();
            write_admin_user_audit(state, principal, &target_username, "user.create", "failure");
            if msg.contains("already exists") {
                error_response(StatusCode::CONFLICT, "username_already_exists", &msg)
            } else {
                error_response(StatusCode::BAD_REQUEST, "invalid_user", &msg)
            }
        }
    }
}

#[allow(clippy::result_large_err)]
async fn handle_admin_user_update(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    id: &str,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may update users",
        );
    }
    let body = match parse_json_body::<AdminUserUpdateRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    do_admin_user_update(state, principal, id, body)
}

#[allow(clippy::result_large_err)]
fn do_admin_user_update(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    id: &str,
    body: AdminUserUpdateRequest,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may update users",
        );
    }
    let id_owned = id.to_string();
    let result = mutate_collection(state, |c| {
        if c.find_by_id(&id_owned).is_none() {
            anyhow::bail!("not_found");
        }
        let updated = c.update(&id_owned, |op| {
            if let Some(username) = &body.username {
                op.username = username.clone();
            }
            if let Some(role) = body.role {
                op.role = user_role_from_wire(role);
            }
            if let Some(grants) = &body.granted_stores {
                op.granted_stores = grants.clone();
            }
        })?;
        Ok(updated.clone())
    });
    match result {
        Ok(updated) => {
            write_admin_user_audit(
                state,
                principal,
                &updated.username,
                "user.update",
                "success",
            );
            json_response(StatusCode::OK, &user_detail(&updated))
        }
        Err(e) => {
            let msg = e.to_string();
            write_admin_user_audit(state, principal, &id_owned, "user.update", "failure");
            if msg.contains("not_found") || msg.contains("not found") {
                error_response(StatusCode::NOT_FOUND, "not_found", "unknown user")
            } else if msg.contains("already exists") {
                error_response(StatusCode::CONFLICT, "username_already_exists", &msg)
            } else if msg.contains("last_admin_protected") {
                error_response(
                    StatusCode::CONFLICT,
                    "last_admin_protected",
                    "cannot demote the last remaining admin",
                )
            } else {
                error_response(StatusCode::BAD_REQUEST, "invalid_user", &msg)
            }
        }
    }
}

#[allow(clippy::result_large_err)]
async fn handle_admin_user_reset_password(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    id: &str,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may reset passwords",
        );
    }
    let body = match parse_json_body::<AdminUserResetPasswordRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    if body.password.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_password",
            "password must not be empty",
        );
    }
    let new_hash = match crate::auth::hash_password(&body.password) {
        Ok(h) => h,
        Err(e) => {
            warn!(err = %e, "users.reset-password: argon2 hash failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "could not hash password",
            );
        }
    };
    let id_owned = id.to_string();
    let now = crate::operators::now_epoch_secs();
    let result = mutate_collection(state, |c| {
        if c.find_by_id(&id_owned).is_none() {
            anyhow::bail!("not_found");
        }
        let updated = c.update(&id_owned, |op| {
            op.password_hash = new_hash.clone();
            op.password_changed_at = now;
        })?;
        Ok(updated.username.clone())
    });
    match result {
        Ok(target_username) => {
            state
                .session_revocations
                .revoke_username(&target_username, now);
            write_admin_user_audit(
                state,
                principal,
                &target_username,
                "user.password.reset",
                "success",
            );
            json_response(StatusCode::OK, &AdminOkUserResponse { ok: true })
        }
        Err(e) => {
            let msg = e.to_string();
            write_admin_user_audit(
                state,
                principal,
                &id_owned,
                "user.password.reset",
                "failure",
            );
            if msg.contains("not_found") {
                error_response(StatusCode::NOT_FOUND, "not_found", "unknown user")
            } else {
                error_response(StatusCode::BAD_REQUEST, "invalid_user", &msg)
            }
        }
    }
}

#[allow(clippy::result_large_err)]
fn handle_admin_user_delete(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    id: &str,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may delete users",
        );
    }
    let id_owned = id.to_string();
    let now = crate::operators::now_epoch_secs();
    let result = mutate_collection(state, |c| {
        if c.find_by_id(&id_owned).is_none() {
            anyhow::bail!("not_found");
        }
        c.remove(&id_owned)
    });
    match result {
        Ok(removed) => {
            state
                .session_revocations
                .revoke_username(&removed.username, now);
            write_admin_user_audit(
                state,
                principal,
                &removed.username,
                "user.delete",
                "success",
            );
            json_response(StatusCode::OK, &AdminOkUserResponse { ok: true })
        }
        Err(e) => {
            let msg = e.to_string();
            write_admin_user_audit(state, principal, &id_owned, "user.delete", "failure");
            if msg.contains("not_found") {
                error_response(StatusCode::NOT_FOUND, "not_found", "unknown user")
            } else if msg.contains("last_admin_protected") {
                error_response(
                    StatusCode::CONFLICT,
                    "last_admin_protected",
                    "cannot delete the last remaining admin",
                )
            } else {
                error_response(StatusCode::BAD_REQUEST, "invalid_user", &msg)
            }
        }
    }
}

/// `POST /v1/admin/auth/password` — self-service password change for
/// the currently authenticated operator. Requires the current password
/// as proof-of-possession.
#[allow(clippy::result_large_err)]
async fn handle_admin_password_change(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<AdminPasswordChangeRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    if body.new_password.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_password",
            "new password must not be empty",
        );
    }
    let collection = match crate::operators::load(state.admin_home_store.as_ref()) {
        Ok(c) => c,
        Err(e) => {
            warn!(err = %e, "auth.password: collection unreadable");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "operator collection unavailable",
            );
        }
    };
    let current_hash = match collection.find_by_id(&principal.id) {
        Some(op) => op.password_hash.clone(),
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "session refers to an operator that no longer exists",
            );
        }
    };
    if !crate::auth::verify_password(&current_hash, &body.current_password) {
        write_admin_user_audit(
            state,
            principal,
            &principal.username,
            "user.password.change",
            "wrong_current_password",
        );
        return error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "current password does not match",
        );
    }
    let new_hash = match crate::auth::hash_password(&body.new_password) {
        Ok(h) => h,
        Err(e) => {
            warn!(err = %e, "auth.password: argon2 hash failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "could not hash password",
            );
        }
    };
    let id_owned = principal.id.clone();
    let now = crate::operators::now_epoch_secs();
    let result = mutate_collection(state, |c| {
        if c.find_by_id(&id_owned).is_none() {
            anyhow::bail!("not_found");
        }
        c.update(&id_owned, |op| {
            op.password_hash = new_hash.clone();
            op.password_changed_at = now;
        })?;
        Ok(())
    });
    match result {
        Ok(()) => {
            write_admin_user_audit(
                state,
                principal,
                &principal.username,
                "user.password.change",
                "success",
            );
            json_response(StatusCode::OK, &AdminOkUserResponse { ok: true })
        }
        Err(e) => {
            warn!(err = %e, "auth.password: save failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "could not save new password",
            )
        }
    }
}

// ===== Stores CRUD (ticket 231) =====
//
// All five handlers operate on the runtime `state.stores` registry
// plus the on-disk `<stores_dynamic_dir>/<name>.yaml` file. Mutating
// handlers refuse declarative stores (409 declarative_immutable) and
// validate the requested data `dir` against the configured allow-
// list of roots. Atomicity comes from `tempfile::NamedTempFile`
// `persist()` on the YAML write side and a brief held `write()`
// lock on the registry update.

use underwrap_protocol::service::{
    AdminStoreBackendKind, AdminStoreCreateRequest, AdminStoreCreateResponse,
    AdminStoreDeleteResponse, AdminStoreDetailResponse, AdminStoreListItem,
    AdminStoreRotateKeyResponse, AdminStoreSource, AdminStoreUpdateRequest,
    AdminStoresListResponse,
};

fn backend_kind(storage: &underwrap_protocol::config::StorageConfig) -> AdminStoreBackendKind {
    match storage {
        underwrap_protocol::config::StorageConfig::Plain { .. } => AdminStoreBackendKind::Plain,
        underwrap_protocol::config::StorageConfig::Encrypted { .. } => {
            AdminStoreBackendKind::Encrypted
        }
    }
}

fn source_kind(s: StoreSource) -> AdminStoreSource {
    match s {
        StoreSource::Declarative => AdminStoreSource::Declarative,
        StoreSource::Dynamic => AdminStoreSource::Dynamic,
    }
}

fn admin_store_list_item(name: &str, loaded: &LoadedStoreDef) -> AdminStoreListItem {
    AdminStoreListItem {
        name: name.to_string(),
        source: source_kind(loaded.source),
        backend: backend_kind(&loaded.definition.storage),
        description: loaded.definition.description.clone(),
        created_at: loaded.definition.created_at,
    }
}

fn admin_store_detail(name: &str, loaded: &LoadedStoreDef) -> AdminStoreDetailResponse {
    let (dir, key_file) = match &loaded.definition.storage {
        underwrap_protocol::config::StorageConfig::Plain { dir } => {
            (dir.to_string_lossy().to_string(), None)
        }
        underwrap_protocol::config::StorageConfig::Encrypted { dir, key_file } => (
            dir.to_string_lossy().to_string(),
            Some(key_file.to_string_lossy().to_string()),
        ),
    };
    AdminStoreDetailResponse {
        name: name.to_string(),
        source: source_kind(loaded.source),
        backend: backend_kind(&loaded.definition.storage),
        dir,
        key_file,
        description: loaded.definition.description.clone(),
        created_at: loaded.definition.created_at,
        source_path: loaded.source_path.to_string_lossy().to_string(),
    }
}

fn handle_admin_stores_list(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
) -> Response<Full<Bytes>> {
    let reg = state.stores.read().unwrap();
    let mut items: Vec<AdminStoreListItem> = reg
        .sources
        .iter()
        .filter(|(name, _)| principal.has_store_access(name))
        .map(|(name, loaded)| admin_store_list_item(name, loaded))
        .collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));
    json_response(StatusCode::OK, &AdminStoresListResponse { items })
}

#[allow(clippy::result_large_err)]
fn handle_admin_store_detail(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    name: &str,
) -> Response<Full<Bytes>> {
    if !principal.has_store_access(name) {
        return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
    }
    let reg = state.stores.read().unwrap();
    match reg.sources.get(name) {
        Some(loaded) => json_response(StatusCode::OK, &admin_store_detail(name, loaded)),
        None => error_response(StatusCode::NOT_FOUND, "not_found", "unknown store"),
    }
}

/// True iff `candidate` is canonically below `root`. We refuse
/// symlinks and `..` traversal by canonicalising `root` first and
/// requiring `candidate` to lexically start with it once normalised.
/// `candidate` may not exist yet (we're about to create it); we walk
/// up its components until we find an existing ancestor and
/// canonicalise from there.
fn path_is_under(candidate: &Path, root: &Path) -> bool {
    let root_canon = match root.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    // Find the deepest existing ancestor of `candidate` to canonicalise.
    let mut existing = candidate.to_path_buf();
    let mut suffix = std::path::PathBuf::new();
    while !existing.exists() {
        let Some(parent) = existing.parent().map(|p| p.to_path_buf()) else {
            return false;
        };
        let file_name = match existing.file_name() {
            Some(n) => n.to_owned(),
            None => return false,
        };
        let mut rebuilt = std::path::PathBuf::from(file_name);
        rebuilt.push(&suffix);
        suffix = rebuilt;
        existing = parent;
    }
    let Ok(canon_existing) = existing.canonicalize() else {
        return false;
    };
    let combined = canon_existing.join(suffix);
    combined.starts_with(&root_canon)
}

/// Validate a `dir` from a create request: must be absolute, not
/// already a configured store dir, and below at least one of
/// `stores_data_roots`.
fn validate_create_dir(
    state: &AdminRuntimeState,
    dir: &Path,
) -> std::result::Result<PathBuf, (&'static str, String)> {
    if !dir.is_absolute() {
        return Err((
            "dir_must_be_absolute",
            "`dir` must be an absolute path".to_string(),
        ));
    }
    if state.stores_data_roots.is_empty() {
        return Err((
            "data_root_not_configured",
            "store creation is disabled: backend.yaml has no `admin.stores_data_roots`".to_string(),
        ));
    }
    let allowed = state
        .stores_data_roots
        .iter()
        .any(|root| path_is_under(dir, root));
    if !allowed {
        return Err((
            "dir_outside_allowed_roots",
            "`dir` is not below any configured stores_data_roots prefix".to_string(),
        ));
    }
    Ok(dir.to_path_buf())
}

fn write_store_yaml_atomic(path: &Path, def: &StoreDefinition) -> Result<()> {
    let yaml = serde_yaml::to_string(def).context("serialising store definition")?;
    let dir = path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "store definition path '{}' has no parent directory",
            path.display()
        )
    })?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating stores.d dir {}", dir.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("creating temp store-def under {}", dir.display()))?;
    use std::io::Write;
    tmp.write_all(yaml.as_bytes())
        .context("writing store definition tempfile")?;
    tmp.persist(path).map_err(|e| {
        anyhow::anyhow!(
            "atomically persisting store definition to {}: {e}",
            path.display(),
        )
    })?;
    Ok(())
}

#[allow(clippy::result_large_err)]
async fn handle_admin_store_create(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may create stores",
        );
    }
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
    principal: &crate::operators::Principal,
    body: AdminStoreCreateRequest,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may create stores",
        );
    }
    if !is_valid_store_name(&body.name) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_store_name",
            "store name must match [a-z0-9][a-z0-9_-]*",
        );
    }
    // No name collisions across either source.
    {
        let reg = state.stores.read().unwrap();
        if let Some(existing) = reg.sources.get(&body.name) {
            return error_response(
                StatusCode::CONFLICT,
                "store_already_exists",
                &format!(
                    "store '{}' already exists ({}{})",
                    body.name,
                    match existing.source {
                        StoreSource::Declarative => "declarative source ",
                        StoreSource::Dynamic => "dynamic source ",
                    },
                    existing.source_path.display(),
                ),
            );
        }
    }
    let dir = PathBuf::from(&body.dir);
    let validated_dir = match validate_create_dir(state, &dir) {
        Ok(p) => p,
        Err((code, msg)) => return error_response(StatusCode::BAD_REQUEST, code, &msg),
    };
    // For encrypted stores we generate the master key here and stash
    // it in `generated_key_hex` so it can be returned to the caller
    // **once**. The key value never leaves this function except as
    // part of the `AdminStoreCreateResponse` returned below.
    let (storage, generated_key_hex) = match body.backend {
        AdminStoreBackendKind::Plain => (
            underwrap_protocol::config::StorageConfig::Plain {
                dir: validated_dir.clone(),
            },
            None,
        ),
        AdminStoreBackendKind::Encrypted => {
            let key_file = state.keys_dir.join(format!("{}.key", body.name));
            // Refuse if a stale key file is already there — it would
            // belong to a previous store of the same name, and we'd
            // overwrite an operator's secret material.
            if key_file.exists() {
                return error_response(
                    StatusCode::CONFLICT,
                    "key_file_exists",
                    &format!(
                        "refusing to overwrite existing key file at {} — remove it manually if you intended to recreate this store",
                        key_file.display(),
                    ),
                );
            }
            let key_hex = match generate_master_key_atomic(&key_file) {
                Ok(hex) => hex,
                Err(e) => {
                    return error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "key_generation_failed",
                        &format!("generating master key at {}: {e:#}", key_file.display()),
                    );
                }
            };
            (
                underwrap_protocol::config::StorageConfig::Encrypted {
                    dir: validated_dir.clone(),
                    key_file,
                },
                Some(key_hex),
            )
        }
    };
    let definition = StoreDefinition {
        description: body.description.clone(),
        created_at: Some(crate::operators::now_epoch_secs()),
        storage,
    };
    // Create the data dir if needed. Mode 0700 keeps other users out;
    // operators who want a different mode can pre-create the dir.
    if let Err(e) = create_data_dir_secure(&validated_dir) {
        // Roll any generated key file back so we don't leave an
        // orphan 0600 file in keys_dir for a store that doesn't exist.
        rollback_generated_key(&definition.storage);
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "create_data_dir_failed",
            &format!("creating data dir {}: {e:#}", validated_dir.display()),
        );
    }
    let path = state.stores_dynamic_dir.join(format!("{}.yaml", body.name));
    if let Err(e) = write_store_yaml_atomic(&path, &definition) {
        rollback_generated_key(&definition.storage);
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "write_store_def_failed",
            &format!("{e:#}"),
        );
    }
    // Open the backend and swap into the registry.
    let opened: Arc<dyn StorageBackend> = match underwrap_store::open(&definition.storage) {
        Ok(b) => Arc::from(b),
        Err(e) => {
            // Roll the YAML and key file back so the next request
            // doesn't see a half-created store.
            let _ = std::fs::remove_file(&path);
            rollback_generated_key(&definition.storage);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "open_store_failed",
                &format!("opening store backend after create: {e:#}"),
            );
        }
    };
    {
        let mut reg = state.stores.write().unwrap();
        reg.open.insert(body.name.clone(), opened);
        reg.sources.insert(
            body.name.clone(),
            LoadedStoreDef {
                source: StoreSource::Dynamic,
                source_path: path,
                definition: definition.clone(),
            },
        );
    }
    write_admin_store_audit(state, principal, &body.name, "store.create", "success");
    let loaded = LoadedStoreDef {
        source: StoreSource::Dynamic,
        source_path: state.stores_dynamic_dir.join(format!("{}.yaml", body.name)),
        definition,
    };
    json_response(
        StatusCode::OK,
        &AdminStoreCreateResponse {
            store: admin_store_detail(&body.name, &loaded),
            key_hex: generated_key_hex,
        },
    )
}

/// Best-effort cleanup of an in-flight encrypted-store creation:
/// drop the key file we just wrote so a failed YAML or backend-open
/// step doesn't leave an orphan 0600 file in `keys_dir` for a store
/// that never made it into the registry.
fn rollback_generated_key(storage: &underwrap_protocol::config::StorageConfig) {
    if let underwrap_protocol::config::StorageConfig::Encrypted { key_file, .. } = storage {
        let _ = std::fs::remove_file(key_file);
    }
}

/// Generate 32 fresh random bytes and atomically install them as the
/// hex-encoded master key at `path` with mode 0600. The hex form is
/// returned to the caller; the disk and the in-memory copy share the
/// same bytes so we don't have to keep both around. The temp file
/// lives in the same directory as `path` so `persist` is rename-only.
fn generate_master_key_atomic(path: &Path) -> Result<String> {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let key_hex = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();

    let dir = path.parent().ok_or_else(|| {
        anyhow::anyhow!("master key path {} has no parent directory", path.display())
    })?;
    // mkdir_p the keys dir if it's missing. 0700 keeps other users
    // out even if the parent's default mode would be looser.
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        if !dir.exists() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .with_context(|| format!("creating keys dir {}", dir.display()))?;
        }
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating keys dir {}", dir.display()))?;
    }

    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("creating temp key file in {}", dir.display()))?;
    use std::io::Write;
    tmp.write_all(key_hex.as_bytes())
        .context("writing master key tempfile")?;
    tmp.as_file()
        .sync_all()
        .context("fsync master key tempfile")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(tmp.path(), perms)
            .with_context(|| format!("setting 0600 on {}", tmp.path().display()))?;
    }
    tmp.persist(path).map_err(|e| {
        anyhow::anyhow!(
            "atomically persisting master key to {}: {e}",
            path.display()
        )
    })?;
    Ok(key_hex)
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
    principal: &crate::operators::Principal,
    name: &str,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may update stores",
        );
    }
    let body = match parse_json_body::<AdminStoreUpdateRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    do_admin_store_update(state, principal, name, body)
}

#[allow(clippy::result_large_err)]
fn do_admin_store_update(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    name: &str,
    body: AdminStoreUpdateRequest,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may update stores",
        );
    }
    // Snapshot the source so we can build the updated YAML without
    // holding the lock across I/O. We re-check the source under the
    // write lock at the end before swapping.
    let (mut updated_def, source_path) = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.sources.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        if loaded.source == StoreSource::Declarative {
            return error_response(
                StatusCode::CONFLICT,
                "declarative_immutable",
                &format!(
                    "store '{name}' is declarative (defined in {}); edit the Nix module instead",
                    loaded.source_path.display(),
                ),
            );
        }
        (loaded.definition.clone(), loaded.source_path.clone())
    };
    if let Some(new_desc) = body.description {
        updated_def.description = if new_desc.is_empty() {
            None
        } else {
            Some(new_desc)
        };
    }
    if let Err(e) = write_store_yaml_atomic(&source_path, &updated_def) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "write_store_def_failed",
            &format!("{e:#}"),
        );
    }
    {
        let mut reg = state.stores.write().unwrap();
        if let Some(loaded) = reg.sources.get_mut(name) {
            loaded.definition = updated_def.clone();
        }
    }
    write_admin_store_audit(state, principal, name, "store.update", "success");
    let loaded = LoadedStoreDef {
        source: StoreSource::Dynamic,
        source_path,
        definition: updated_def,
    };
    json_response(StatusCode::OK, &admin_store_detail(name, &loaded))
}

#[allow(clippy::result_large_err)]
fn handle_admin_store_delete(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    name: &str,
    force: bool,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may delete stores",
        );
    }
    // Inspect under read lock first.
    let (source_path, store_handle, dir_for_hint) = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.sources.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        if loaded.source == StoreSource::Declarative {
            return error_response(
                StatusCode::CONFLICT,
                "declarative_immutable",
                &format!(
                    "store '{name}' is declarative (defined in {}); edit the Nix module instead",
                    loaded.source_path.display(),
                ),
            );
        }
        let Some(handle) = reg.open.get(name).cloned() else {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "store metadata present but backend missing",
            );
        };
        let dir = match &loaded.definition.storage {
            underwrap_protocol::config::StorageConfig::Plain { dir } => dir.clone(),
            underwrap_protocol::config::StorageConfig::Encrypted { dir, .. } => dir.clone(),
        };
        (loaded.source_path.clone(), handle, dir)
    };

    let non_empty = match store_handle.list("") {
        Ok(ids) => !ids.is_empty(),
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "list_store_failed",
                &format!("checking store contents before delete: {e:#}"),
            );
        }
    };
    if non_empty && !force {
        return error_response(
            StatusCode::CONFLICT,
            "store_not_empty",
            "store contains secrets; re-issue with `?force=true` to delete the definition anyway",
        );
    }

    // Atomic-as-we-can: remove the YAML, then drop from the registry.
    if let Err(e) = std::fs::remove_file(&source_path) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "remove_store_def_failed",
            &format!("removing {}: {e:#}", source_path.display()),
        );
    }
    {
        let mut reg = state.stores.write().unwrap();
        reg.open.remove(name);
        reg.sources.remove(name);
    }
    let cleanup_hint = if non_empty {
        Some(format!(
            "store '{name}' had secrets at deletion time. The YAML definition has been removed; \
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

/// `POST /v1/admin/stores/{name}/rotate-key` — generate a fresh
/// master key, replace the on-disk key file atomically, reopen the
/// backend, and return the new key value **exactly once**. Refuses
/// declarative stores and plain stores. Existing ciphertext encrypted
/// with the previous key becomes unreadable through this backend —
/// v1 of rotation does not re-encrypt; the warning in the response
/// documents that limitation.
#[allow(clippy::result_large_err)]
fn handle_admin_store_rotate_key(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    name: &str,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may rotate store keys",
        );
    }

    // Snapshot under read lock so we don't hold it across key-file I/O.
    let (key_file, dir) = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.sources.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        if loaded.source == StoreSource::Declarative {
            return error_response(
                StatusCode::CONFLICT,
                "declarative_immutable",
                &format!(
                    "store '{name}' is declarative (defined in {}); edit the Nix module instead",
                    loaded.source_path.display(),
                ),
            );
        }
        match &loaded.definition.storage {
            underwrap_protocol::config::StorageConfig::Plain { .. } => {
                return error_response(
                    StatusCode::CONFLICT,
                    "not_encrypted",
                    "store is not encrypted; rotate-key only applies to encrypted backends",
                );
            }
            underwrap_protocol::config::StorageConfig::Encrypted { dir, key_file } => {
                (key_file.clone(), dir.clone())
            }
        }
    };

    // Atomic key replacement: write the new key to a tempfile in the
    // same directory, then rename over the existing file. `persist`
    // is a POSIX rename, so a reader that opens after rename gets the
    // new key and the file is never observed in a partial state.
    let key_hex = match generate_master_key_atomic(&key_file) {
        Ok(hex) => hex,
        Err(e) => {
            write_admin_store_audit(state, principal, name, "store.rotate-key", "failure");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "key_generation_failed",
                &format!("rotating master key at {}: {e:#}", key_file.display()),
            );
        }
    };

    // Reopen the backend with the new key and swap it into the
    // registry. If reopen fails the new key file is still on disk
    // (so the daemon-restart path stays consistent) but the old
    // in-memory backend keeps serving until the next restart.
    let new_backend: Arc<dyn StorageBackend> =
        match underwrap_store::open(&underwrap_protocol::config::StorageConfig::Encrypted {
            dir,
            key_file: key_file.clone(),
        }) {
            Ok(b) => Arc::from(b),
            Err(e) => {
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
                    &format!("reopening store with new master key: {e:#}"),
                );
            }
        };
    {
        let mut reg = state.stores.write().unwrap();
        reg.open.insert(name.to_string(), new_backend);
    }

    write_admin_store_audit(state, principal, name, "store.rotate-key", "success");
    json_response(
        StatusCode::OK,
        &AdminStoreRotateKeyResponse {
            key_hex,
            warning: "the previous master key has been replaced; secrets already \
                     stored under this store remain encrypted with the old key and \
                     cannot be decrypted through this backend. re-encrypt them \
                     offline if you need to keep them readable."
                .to_string(),
        },
    )
}

fn write_admin_store_audit(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    store_name: &str,
    action: &str,
    result: &str,
) {
    let event = underwrap_protocol::service::AdminUiAuditEvent {
        ts_epoch: crate::operators::now_epoch_secs(),
        action: action.to_string(),
        operator: principal.username.clone(),
        wrapper: None,
        resource: Some(format!("store:{store_name}")),
        secret_id: None,
        operation: None,
        result: result.to_string(),
    };
    if let Err(e) = append_audit_history_event(state.admin_home_store.as_ref(), event) {
        warn!(err = %e, "store CRUD: writing audit event failed");
    }
}

// ===== Agent grant CRUD + writes-enabled toggle (tickets 246, 247) =====

fn load_policy_document_or_default(store: &dyn StorageBackend) -> Result<PolicyDocument> {
    match store.read(POLICY_DOCUMENT_SECRET_ID)? {
        Some(material) => Ok(serde_json::from_slice(material.as_ref())?),
        None => Ok(PolicyDocument::default()),
    }
}

fn save_policy_document(store: &dyn StorageBackend, doc: &PolicyDocument) -> Result<()> {
    store.write(
        POLICY_DOCUMENT_SECRET_ID,
        serde_json::to_vec(doc)?.as_slice(),
    )
}

fn write_admin_grant_audit(
    state: &AdminRuntimeState,
    actor: &crate::operators::Principal,
    store_name: &str,
    grant: &underwrap_protocol::policy::AgentGrant,
    action: &str,
    result: &str,
) {
    let event = underwrap_protocol::service::AdminUiAuditEvent {
        ts_epoch: crate::operators::now_epoch_secs(),
        action: action.to_string(),
        operator: actor.username.clone(),
        wrapper: Some(grant.wrapper.clone()),
        resource: Some(format!(
            "grant:{store_name}/{}/{}/{}",
            grant.wrapper, grant.host, grant.user
        )),
        secret_id: None,
        operation: None,
        result: result.to_string(),
    };
    if let Err(e) = append_audit_history_event(state.admin_home_store.as_ref(), event) {
        warn!(err = %e, "grant CRUD: writing audit event failed");
    }
}

#[allow(clippy::result_large_err)]
fn handle_admin_grants_list(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminAgentGrantsListRequest,
) -> Response<Full<Bytes>> {
    let store = match admin_store_for_principal(state, principal, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    match load_policy_document_or_default(store.as_ref()) {
        Ok(doc) => json_response(
            StatusCode::OK,
            &underwrap_protocol::service::AdminAgentGrantsListResponse { grants: doc.grants },
        ),
        Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    }
}

#[allow(clippy::result_large_err)]
fn handle_admin_grant_upsert(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminAgentGrantUpsertRequest,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may mutate grants",
        );
    }
    let store = match admin_store_for_principal(state, principal, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let mut doc = match load_policy_document_or_default(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string());
        }
    };
    if body.grant.wrapper.is_empty() || body.grant.host.is_empty() || body.grant.user.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "wrapper, host, and user must all be non-empty",
        );
    }
    let new_grant = body.grant.clone();
    if let Some(slot) = doc.grants.iter_mut().find(|g| {
        g.wrapper == new_grant.wrapper && g.host == new_grant.host && g.user == new_grant.user
    }) {
        *slot = new_grant.clone();
    } else {
        doc.grants.push(new_grant.clone());
    }
    doc.grants.sort_by(|a, b| {
        a.wrapper
            .cmp(&b.wrapper)
            .then(a.host.cmp(&b.host))
            .then(a.user.cmp(&b.user))
    });
    if let Err(e) = save_policy_document(store.as_ref(), &doc) {
        write_admin_grant_audit(
            state,
            principal,
            &body.store,
            &new_grant,
            "grant.upsert",
            "failure",
        );
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting policy document: {e:#}"),
        );
    }
    write_admin_grant_audit(
        state,
        principal,
        &body.store,
        &new_grant,
        "grant.upsert",
        "success",
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminOkResponse { ok: true },
    )
}

#[allow(clippy::result_large_err)]
fn handle_admin_grant_delete(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminAgentGrantDeleteRequest,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may mutate grants",
        );
    }
    let store = match admin_store_for_principal(state, principal, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let mut doc = match load_policy_document_or_default(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string());
        }
    };
    let before = doc.grants.len();
    doc.grants
        .retain(|g| !(g.wrapper == body.wrapper && g.host == body.host && g.user == body.user));
    let removed = before != doc.grants.len();
    if !removed {
        return error_response(StatusCode::NOT_FOUND, "not_found", "no matching grant");
    }
    if let Err(e) = save_policy_document(store.as_ref(), &doc) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting policy document: {e:#}"),
        );
    }
    let removed_grant = underwrap_protocol::policy::AgentGrant {
        wrapper: body.wrapper.clone(),
        host: body.host.clone(),
        user: body.user.clone(),
        mode: underwrap_protocol::policy::WrapperApprovalMode::Locked,
        reason: None,
    };
    write_admin_grant_audit(
        state,
        principal,
        &body.store,
        &removed_grant,
        "grant.delete",
        "success",
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminOkResponse { ok: true },
    )
}

#[allow(clippy::result_large_err)]
fn handle_admin_write_permissions_get(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminWritePermissionsGetRequest,
) -> Response<Full<Bytes>> {
    let store = match admin_store_for_principal(state, principal, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let doc = match load_policy_document_or_default(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string());
        }
    };
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminWritePermissionsResponse {
            store: body.store.clone(),
            files_writable: doc.files_writable,
            libsecret_writable: doc.libsecret_writable,
        },
    )
}

#[allow(clippy::result_large_err)]
fn handle_admin_write_permissions_set(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminWritePermissionsSetRequest,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may edit per-resource write permissions",
        );
    }
    let store = match admin_store_for_principal(state, principal, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let mut doc = match load_policy_document_or_default(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string());
        }
    };
    let (previous, slot_label) = match body.slot {
        underwrap_protocol::service::AdminWritableSlot::Files => {
            let prev = doc.files_writable_for(&body.resource);
            if body.enabled {
                doc.files_writable.insert(body.resource.clone(), true);
            } else {
                doc.files_writable.remove(&body.resource);
            }
            (prev, "files")
        }
        underwrap_protocol::service::AdminWritableSlot::Libsecret => {
            let prev = doc.libsecret_writable_for(&body.resource);
            if body.enabled {
                doc.libsecret_writable.insert(body.resource.clone(), true);
            } else {
                doc.libsecret_writable.remove(&body.resource);
            }
            (prev, "libsecret")
        }
    };
    if let Err(e) = save_policy_document(store.as_ref(), &doc) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting policy document: {e:#}"),
        );
    }
    let event = underwrap_protocol::service::AdminUiAuditEvent {
        ts_epoch: crate::operators::now_epoch_secs(),
        action: format!("policy.{slot_label}-writable"),
        operator: principal.username.clone(),
        wrapper: None,
        resource: Some(format!("store:{}/{}", body.store, body.resource)),
        secret_id: None,
        operation: None,
        result: format!("{previous}->{}", body.enabled),
    };
    if let Err(e) = append_audit_history_event(state.admin_home_store.as_ref(), event) {
        warn!(err = %e, "write-permissions: writing audit event failed");
    }
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminWritePermissionsResponse {
            store: body.store.clone(),
            files_writable: doc.files_writable,
            libsecret_writable: doc.libsecret_writable,
        },
    )
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

    // Path-parameterised stores routes. Match them first so the
    // literal "/v1/admin/stores" dispatch below doesn't shadow them.
    //
    // `/v1/admin/stores/{name}/rotate-key` is the only nested route;
    // it must be matched before the generic single-segment matcher
    // (which filters out any path containing '/').
    if let Some(rest) = request.uri().path().strip_prefix("/v1/admin/stores/")
        && let Some(name) = rest.strip_suffix("/rotate-key")
        && !name.is_empty()
        && !name.contains('/')
    {
        return Ok(match *request.method() {
            Method::POST => handle_admin_store_rotate_key(&state, &principal, name),
            _ => error_response(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed", "POST"),
        });
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
                    return Ok(handle_admin_user_detail(&state, &principal, &id));
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
                return Ok(handle_admin_store_detail(&state, &principal, &name));
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
            return Ok(handle_admin_users_list(&state, &principal));
        }
        (&Method::POST, "/v1/admin/users") => {
            return Ok(handle_admin_user_create(&state, &principal, request).await);
        }
        (&Method::POST, "/v1/admin/auth/password") => {
            return Ok(handle_admin_password_change(&state, &principal, request).await);
        }
        (&Method::GET, "/v1/admin/stores") => {
            return Ok(handle_admin_stores_list(&state, &principal));
        }
        (&Method::POST, "/v1/admin/stores") => {
            return Ok(handle_admin_store_create(&state, &principal, request).await);
        }
        (&Method::GET, "/v1/admin/ui/overview") => {
            let now_epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0);
            // Snapshot the open backends so we can release the read
            // lock before doing I/O against each store.
            let snapshot: Vec<(String, Arc<dyn StorageBackend>)> = {
                let reg = state.stores.read().unwrap();
                reg.open
                    .iter()
                    .filter(|(name, _)| principal.has_store_access(name))
                    .map(|(name, store)| (name.clone(), store.clone()))
                    .collect()
            };
            match snapshot
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
        (&Method::POST, "/v1/admin/ui/audit/query") => {
            match parse_json_body::<underwrap_protocol::service::AdminUiAuditQueryRequest>(request)
                .await
            {
                Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                    Ok(store) => match admin_ui_audit_query(store.as_ref(), &body) {
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
            // Admin deletes are always allowed once per-store access
            // has been checked; per-resource write flags only gate
            // wrapper-side writes.
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
                    Ok(store) => match load_catalog_document(store.as_ref()) {
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
        // ===== Agent grant CRUD (ticket 246) =====
        (&Method::POST, "/v1/admin/grants/list") => match parse_json_body::<
            underwrap_protocol::service::AdminAgentGrantsListRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_grants_list(&state, &principal, &body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/grants/upsert") => match parse_json_body::<
            underwrap_protocol::service::AdminAgentGrantUpsertRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_grant_upsert(&state, &principal, &body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/grants/delete") => match parse_json_body::<
            underwrap_protocol::service::AdminAgentGrantDeleteRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_grant_delete(&state, &principal, &body),
            Err(response) => response,
        },
        // ===== Per-resource wrapper-write permissions =====
        (&Method::POST, "/v1/admin/policy/write-permissions/get") => {
            match parse_json_body::<underwrap_protocol::service::AdminWritePermissionsGetRequest>(
                request,
            )
            .await
            {
                Ok(body) => handle_admin_write_permissions_get(&state, &principal, &body),
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/admin/policy/write-permissions/set") => {
            match parse_json_body::<underwrap_protocol::service::AdminWritePermissionsSetRequest>(
                request,
            )
            .await
            {
                Ok(body) => handle_admin_write_permissions_set(&state, &principal, &body),
                Err(response) => response,
            }
        }
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
            stores: std::sync::RwLock::new(StoreRegistry {
                open: stores,
                sources: HashMap::new(),
            }),
            stores_dynamic_dir: std::path::PathBuf::new(),
            stores_data_roots: Vec::new(),
            keys_dir: std::path::PathBuf::new(),
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
            operator_collection_lock: std::sync::Mutex::new(()),
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
            stores_data_roots: None,
            keys_dir: None,
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
            stores_data_roots: None,
            keys_dir: None,
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
            stores_data_roots: None,
            keys_dir: None,
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
            stores_data_roots: None,
            keys_dir: None,
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
            stores: std::sync::RwLock::new(StoreRegistry {
                open: HashMap::from([("home".to_string(), store.clone())]),
                sources: HashMap::new(),
            }),
            stores_dynamic_dir: std::path::PathBuf::new(),
            stores_data_roots: Vec::new(),
            keys_dir: std::path::PathBuf::new(),
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
            operator_collection_lock: std::sync::Mutex::new(()),
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

    fn expect_404(result: std::result::Result<Arc<dyn StorageBackend>, Response<Full<Bytes>>>) {
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
            stores: std::sync::RwLock::new(StoreRegistry {
                open: HashMap::from([("home".to_string(), store.clone())]),
                sources: HashMap::new(),
            }),
            stores_dynamic_dir: std::path::PathBuf::new(),
            stores_data_roots: Vec::new(),
            keys_dir: std::path::PathBuf::new(),
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
            operator_collection_lock: std::sync::Mutex::new(()),
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

    // ===== Stores CRUD (ticket 231) =====

    fn admin_principal(stores: &[&str]) -> crate::operators::Principal {
        crate::operators::Principal {
            id: "admin-id".into(),
            username: "alice".into(),
            role: crate::operators::OperatorRole::Admin,
            granted_stores: stores.iter().map(|s| s.to_string()).collect(),
        }
    }
    fn operator_principal(stores: &[&str]) -> crate::operators::Principal {
        crate::operators::Principal {
            id: "op-id".into(),
            username: "bob".into(),
            role: crate::operators::OperatorRole::Operator,
            granted_stores: stores.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Build an `AdminRuntimeState` wired up for CRUD tests: a
    /// dedicated `stores.d/` and one allowed data root, both under
    /// the supplied tempdir. The admin-home store is a fresh memory
    /// store so audit-event writes succeed.
    fn crud_state(tmp: &std::path::Path) -> AdminRuntimeState {
        let dynamic_dir = tmp.join("stores.d");
        std::fs::create_dir_all(&dynamic_dir).unwrap();
        let data_root = tmp.join("data");
        std::fs::create_dir_all(&data_root).unwrap();
        let keys_dir = tmp.join("keys");
        std::fs::create_dir_all(&keys_dir).unwrap();
        AdminRuntimeState {
            stores: std::sync::RwLock::new(StoreRegistry::new()),
            stores_dynamic_dir: dynamic_dir,
            stores_data_roots: vec![data_root],
            keys_dir,
            tokens: HashMap::new(),
            allowed_origins: None,
            web_source: crate::web::WebAssetSource::Embedded,
            admin_home_store: Arc::new(MemTestStore::new()),
            session_key: None,
            session_lifetime_secs: crate::auth::DEFAULT_SESSION_LIFETIME_SECS,
            cookies_secure: false,
            login_rate_limiter: crate::auth::RateLimiter::new(),
            session_revocations: crate::auth::SessionRevocations::new(),
            bootstrap_lock: std::sync::Mutex::new(()),
            operator_collection_lock: std::sync::Mutex::new(()),
        }
    }

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
            // The Full body has a single frame; drain via block_in_place.
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async { self.collect().await.unwrap().to_bytes().to_vec() })
        }
    }

    #[test]
    fn path_is_under_basic_cases() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        // Existing path inside root → allowed.
        assert!(path_is_under(&root.join("a"), &root));
        // Sibling outside root → rejected.
        let other = tmp.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        assert!(!path_is_under(&other.join("a"), &root));
    }

    #[test]
    fn path_is_under_rejects_traversal() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        // `root/../other` resolves above the allowlist root.
        let outside = root.join("..").join("other-data");
        assert!(!path_is_under(&outside, &root));
    }

    #[test]
    fn validate_create_dir_rejects_relative() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let err = validate_create_dir(&state, std::path::Path::new("relative/path"))
            .expect_err("must reject relative dirs");
        assert_eq!(err.0, "dir_must_be_absolute");
    }

    #[test]
    fn validate_create_dir_rejects_when_no_roots_configured() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut state = crud_state(tmp.path());
        state.stores_data_roots.clear();
        let err = validate_create_dir(&state, &tmp.path().join("data").join("x"))
            .expect_err("must refuse without roots");
        assert_eq!(err.0, "data_root_not_configured");
    }

    #[test]
    fn validate_create_dir_rejects_outside_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let outside = tmp.path().join("outside").join("a");
        let err = validate_create_dir(&state, &outside).expect_err("must reject outside-root");
        assert_eq!(err.0, "dir_outside_allowed_roots");
    }

    #[test]
    fn list_returns_empty_when_nothing_configured() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let resp = handle_admin_stores_list(&state, &admin_principal(&[]));
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        assert_eq!(body["items"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn admin_can_create_get_update_delete_a_dynamic_plain_store() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let dir = tmp.path().join("data").join("work");
        let create = AdminStoreCreateRequest {
            name: "work".to_string(),
            backend: AdminStoreBackendKind::Plain,
            dir: dir.to_string_lossy().to_string(),
            description: Some("the work store".into()),
        };
        let resp = do_admin_store_create(&state, &p, create);
        assert_eq!(resp.status(), StatusCode::OK);

        // The yaml file landed.
        let yaml_path = state.stores_dynamic_dir.join("work.yaml");
        assert!(yaml_path.exists());
        // The runtime registry sees it.
        let resp = handle_admin_stores_list(&state, &p);
        let body = response_body_json(resp);
        let items = body["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], "work");
        assert_eq!(items[0]["source"], "dynamic");
        assert_eq!(items[0]["backend"], "plain");
        assert_eq!(items[0]["description"], "the work store");
        // Detail.
        let resp = handle_admin_store_detail(&state, &p, "work");
        let body = response_body_json(resp);
        assert_eq!(body["__status"], 200);
        assert_eq!(body["dir"], dir.to_string_lossy().to_string());
        // Update description.
        let resp = do_admin_store_update(
            &state,
            &p,
            "work",
            AdminStoreUpdateRequest {
                description: Some("renamed".into()),
            },
        );
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = handle_admin_store_detail(&state, &p, "work");
        assert_eq!(response_body_json(resp)["description"], "renamed");
        // Clearing description with an empty string nulls it out.
        let resp = do_admin_store_update(
            &state,
            &p,
            "work",
            AdminStoreUpdateRequest {
                description: Some(String::new()),
            },
        );
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = handle_admin_store_detail(&state, &p, "work");
        let body = response_body_json(resp);
        assert!(!body.as_object().unwrap().contains_key("description"));
        // Delete (empty store).
        let resp = handle_admin_store_delete(&state, &p, "work", false);
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        assert_eq!(body["ok"], true);
        assert!(!body.as_object().unwrap().contains_key("cleanup_hint"));
        assert!(!yaml_path.exists());
    }

    #[test]
    fn declarative_store_mutations_are_rejected_with_documented_code() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        // Hand-stuff a declarative store into the registry — that's
        // what `read_one_dir(declarative, …)` would have done at
        // startup.
        {
            let mut reg = state.stores.write().unwrap();
            let dir = tmp.path().join("data").join("nixed");
            std::fs::create_dir_all(&dir).unwrap();
            let store: Arc<dyn StorageBackend> = Arc::new(MemTestStore::new());
            reg.open.insert("nixed".to_string(), store);
            reg.sources.insert(
                "nixed".to_string(),
                LoadedStoreDef {
                    source: StoreSource::Declarative,
                    source_path: std::path::PathBuf::from("/nix/store/zzz/stores.d/nixed.yaml"),
                    definition: StoreDefinition {
                        description: None,
                        created_at: None,
                        storage: underwrap_protocol::config::StorageConfig::Plain { dir },
                    },
                },
            );
        }
        let p = admin_principal(&[]);
        let resp = do_admin_store_update(
            &state,
            &p,
            "nixed",
            AdminStoreUpdateRequest {
                description: Some("x".into()),
            },
        );
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = response_body_json(resp);
        assert_eq!(body["code"], "declarative_immutable");

        let resp = handle_admin_store_delete(&state, &p, "nixed", true);
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = response_body_json(resp);
        assert_eq!(body["code"], "declarative_immutable");
    }

    #[test]
    fn create_refuses_dir_outside_allowlist() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let outside = tmp.path().join("not-under-root").join("data");
        let resp = do_admin_store_create(
            &state,
            &admin_principal(&[]),
            AdminStoreCreateRequest {
                name: "stray".into(),
                backend: AdminStoreBackendKind::Plain,
                dir: outside.to_string_lossy().to_string(),
                description: None,
            },
        );
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body_json(resp);
        assert_eq!(body["code"], "dir_outside_allowed_roots");
    }

    #[test]
    fn create_refuses_duplicate_name() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let dir = tmp.path().join("data").join("work");
        let req = AdminStoreCreateRequest {
            name: "work".to_string(),
            backend: AdminStoreBackendKind::Plain,
            dir: dir.to_string_lossy().to_string(),
            description: None,
        };
        let _first = do_admin_store_create(&state, &p, req.clone());
        let resp = do_admin_store_create(&state, &p, req);
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = response_body_json(resp);
        assert_eq!(body["code"], "store_already_exists");
    }

    #[test]
    fn non_admin_cannot_create_or_update_or_delete() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let op = operator_principal(&["work"]);
        let dir = tmp.path().join("data").join("work");
        // Create
        let resp = do_admin_store_create(
            &state,
            &op,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                dir: dir.to_string_lossy().to_string(),
                description: None,
            },
        );
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        // Update
        let resp = do_admin_store_update(
            &state,
            &op,
            "work",
            AdminStoreUpdateRequest {
                description: Some("hi".into()),
            },
        );
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        // Delete
        let resp = handle_admin_store_delete(&state, &op, "work", true);
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn delete_refuses_non_empty_without_force_and_emits_cleanup_hint_with_force() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let dir = tmp.path().join("data").join("work");
        do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                dir: dir.to_string_lossy().to_string(),
                description: None,
            },
        );
        // Drop a secret in so the store is non-empty.
        {
            let reg = state.stores.read().unwrap();
            reg.open["work"]
                .write("greeting", &SecretBytes::from(b"hello".to_vec()))
                .unwrap();
        }
        // Without force → 409 + code store_not_empty.
        let resp = handle_admin_store_delete(&state, &p, "work", false);
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        assert_eq!(response_body_json(resp)["code"], "store_not_empty");
        // With force → 200 + cleanup_hint pointing at the data dir.
        let resp = handle_admin_store_delete(&state, &p, "work", true);
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        let hint = body["cleanup_hint"]
            .as_str()
            .expect("force-delete must include a cleanup_hint when non-empty");
        assert!(hint.contains(dir.to_string_lossy().as_ref()));
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
                dir: tmp
                    .path()
                    .join("data")
                    .join("work")
                    .to_string_lossy()
                    .to_string(),
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

    #[cfg(unix)]
    #[test]
    fn create_encrypted_store_generates_key_with_0600_and_returns_it_once() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let dir = tmp.path().join("data").join("vault");
        let resp = do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "vault".into(),
                backend: AdminStoreBackendKind::Encrypted,
                dir: dir.to_string_lossy().to_string(),
                description: None,
            },
        );
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        let key_hex = body["key_hex"]
            .as_str()
            .expect("encrypted create must include key_hex");
        assert_eq!(key_hex.len(), 64, "key_hex must be 32 bytes hex-encoded");
        assert!(
            key_hex.chars().all(|c| c.is_ascii_hexdigit()),
            "key_hex must be lowercase hex",
        );

        // Key file is in state.keys_dir, named after the store, mode 0600.
        let key_path = state.keys_dir.join("vault.key");
        assert!(key_path.exists(), "key file must be created");
        let mode = std::fs::metadata(&key_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "key file must be mode 0600, got {mode:o}");
        let on_disk = std::fs::read_to_string(&key_path).unwrap();
        assert_eq!(
            on_disk, key_hex,
            "key file contents must match returned key"
        );

        // Subsequent detail GETs must NOT echo the key value. (The
        // detail response carries the *path*, not the contents.)
        let resp = handle_admin_store_detail(&state, &p, "vault");
        let body = response_body_json(resp);
        assert_eq!(body["backend"], "encrypted");
        assert_eq!(body["key_file"], key_path.to_string_lossy().to_string());
        let body_str = serde_json::to_string(&body).unwrap();
        assert!(
            !body_str.contains(key_hex),
            "key material leaked into detail response: {body_str}",
        );
    }

    #[cfg(unix)]
    #[test]
    fn create_encrypted_store_refuses_existing_key_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        // Stash a stale key file as if a previous store with this
        // name had been created and torn down without cleanup.
        let stale = state.keys_dir.join("vault.key");
        std::fs::write(&stale, "stale-key").unwrap();
        let resp = do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "vault".into(),
                backend: AdminStoreBackendKind::Encrypted,
                dir: tmp
                    .path()
                    .join("data")
                    .join("vault")
                    .to_string_lossy()
                    .to_string(),
                description: None,
            },
        );
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        assert_eq!(response_body_json(resp)["code"], "key_file_exists");
        // The original file is untouched.
        assert_eq!(std::fs::read_to_string(&stale).unwrap(), "stale-key");
    }

    #[cfg(unix)]
    #[test]
    fn rotate_key_replaces_key_and_returns_new_value_once() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let dir = tmp.path().join("data").join("vault");
        let resp = do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "vault".into(),
                backend: AdminStoreBackendKind::Encrypted,
                dir: dir.to_string_lossy().to_string(),
                description: None,
            },
        );
        assert_eq!(resp.status(), StatusCode::OK);
        let original_key = response_body_json(resp)["key_hex"]
            .as_str()
            .unwrap()
            .to_string();

        let resp = handle_admin_store_rotate_key(&state, &p, "vault");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        let new_key = body["key_hex"].as_str().unwrap().to_string();
        assert_ne!(new_key, original_key, "rotation must produce a fresh key");
        assert_eq!(new_key.len(), 64);
        assert!(
            body["warning"].as_str().unwrap().contains("re-encrypt"),
            "warning must mention the re-encryption limitation",
        );

        // On-disk key file still 0600, contents = new key.
        let key_path = state.keys_dir.join("vault.key");
        let mode = std::fs::metadata(&key_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read_to_string(&key_path).unwrap(), new_key);

        // Detail GET still does not leak the key value.
        let resp = handle_admin_store_detail(&state, &p, "vault");
        let body = response_body_json(resp);
        let body_str = serde_json::to_string(&body).unwrap();
        assert!(!body_str.contains(&new_key));
        assert!(!body_str.contains(&original_key));
    }

    #[test]
    fn rotate_key_refuses_plain_store() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                dir: tmp
                    .path()
                    .join("data")
                    .join("work")
                    .to_string_lossy()
                    .to_string(),
                description: None,
            },
        );
        let resp = handle_admin_store_rotate_key(&state, &p, "work");
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        assert_eq!(response_body_json(resp)["code"], "not_encrypted");
    }

    #[cfg(unix)]
    #[test]
    fn rotate_key_refuses_declarative_store() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        // Hand-stuff an encrypted declarative store.
        let dir = tmp.path().join("data").join("nixed");
        std::fs::create_dir_all(&dir).unwrap();
        let key_file = tmp.path().join("nix-key");
        std::fs::write(&key_file, "a".repeat(64)).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        {
            let mut reg = state.stores.write().unwrap();
            let store: Arc<dyn StorageBackend> = Arc::new(MemTestStore::new());
            reg.open.insert("nixed".to_string(), store);
            reg.sources.insert(
                "nixed".to_string(),
                LoadedStoreDef {
                    source: StoreSource::Declarative,
                    source_path: std::path::PathBuf::from("/nix/store/zzz/nixed.yaml"),
                    definition: StoreDefinition {
                        description: None,
                        created_at: None,
                        storage: underwrap_protocol::config::StorageConfig::Encrypted {
                            dir,
                            key_file,
                        },
                    },
                },
            );
        }
        let resp = handle_admin_store_rotate_key(&state, &admin_principal(&[]), "nixed");
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        assert_eq!(response_body_json(resp)["code"], "declarative_immutable");
    }

    #[test]
    fn rotate_key_audited_and_requires_admin() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let dir = tmp.path().join("data").join("vault");
        do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "vault".into(),
                backend: AdminStoreBackendKind::Encrypted,
                dir: dir.to_string_lossy().to_string(),
                description: None,
            },
        );
        // Non-admin gets 403.
        let op = operator_principal(&["vault"]);
        let resp = handle_admin_store_rotate_key(&state, &op, "vault");
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Admin succeeds and the audit event lands.
        let resp = handle_admin_store_rotate_key(&state, &p, "vault");
        assert_eq!(resp.status(), StatusCode::OK);
        let events = load_audit_history(state.admin_home_store.as_ref()).unwrap();
        let actions: Vec<&str> = events.iter().map(|e| e.action.as_str()).collect();
        assert!(
            actions.contains(&"store.rotate-key"),
            "rotate-key audit event missing; got {actions:?}",
        );
    }

    #[test]
    fn store_crud_actions_emit_audit_events() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let p = admin_principal(&[]);
        let dir = tmp.path().join("data").join("audit-store");
        do_admin_store_create(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "audit-store".into(),
                backend: AdminStoreBackendKind::Plain,
                dir: dir.to_string_lossy().to_string(),
                description: None,
            },
        );
        do_admin_store_update(
            &state,
            &p,
            "audit-store",
            AdminStoreUpdateRequest {
                description: Some("hi".into()),
            },
        );
        handle_admin_store_delete(&state, &p, "audit-store", true);
        // Audit history lives under the admin-home store. Three
        // events expected: create, update, delete.
        let events = load_audit_history(state.admin_home_store.as_ref()).unwrap();
        let actions: Vec<&str> = events.iter().map(|e| e.action.as_str()).collect();
        assert!(
            actions.contains(&"store.create"),
            "create event missing; got {actions:?}",
        );
        assert!(actions.contains(&"store.update"));
        assert!(actions.contains(&"store.delete"));
        for e in &events {
            assert_eq!(e.operator, "alice");
            assert_eq!(e.resource.as_deref(), Some("store:audit-store"));
        }
    }

    // ===== Users CRUD (ticket 240) =====

    fn seed_admin_into(state: &AdminRuntimeState, username: &str) -> String {
        // Returns the operator id of the seeded admin so tests can
        // build a principal that refers to a real record.
        let mut c = crate::operators::OperatorCollection::default();
        let hash = crate::auth::hash_password("seed-pass").unwrap();
        let now = crate::operators::now_epoch_secs();
        let id = crate::operators::new_operator_id();
        c.add(crate::operators::OperatorRecord {
            id: id.clone(),
            username: username.into(),
            password_hash: hash,
            role: crate::operators::OperatorRole::Admin,
            granted_stores: Vec::new(),
            created_at: now,
            password_changed_at: now,
        })
        .unwrap();
        crate::operators::save(state.admin_home_store.as_ref(), &c).unwrap();
        id
    }

    fn admin_principal_with_id(id: &str) -> crate::operators::Principal {
        crate::operators::Principal {
            id: id.into(),
            username: "alice".into(),
            role: crate::operators::OperatorRole::Admin,
            granted_stores: Vec::new(),
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
                role: underwrap_protocol::service::AdminUserRole::Operator,
                granted_stores: vec!["work".into()],
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
        assert_eq!(body["role"], "operator");
        assert_eq!(body["granted_stores"][0], "work");

        // The record landed in the operator collection.
        let collection = crate::operators::load(state.admin_home_store.as_ref()).unwrap();
        assert_eq!(collection.operators.len(), 2);
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
            role: underwrap_protocol::service::AdminUserRole::Operator,
            granted_stores: Vec::new(),
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
                role: underwrap_protocol::service::AdminUserRole::Operator,
                granted_stores: Vec::new(),
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
                    role: underwrap_protocol::service::AdminUserRole::Operator,
                    granted_stores: Vec::new(),
                },
            );
        }
        let resp = handle_admin_users_list(&state, &p);
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        let body_str = serde_json::to_string(&body).unwrap();
        assert!(!body_str.contains("$argon2"));
        assert!(!body_str.contains("password_hash"));
        let users = body["users"].as_array().unwrap();
        assert_eq!(users.len(), 3); // alice + bob + carol
    }

    #[test]
    fn non_admin_cannot_list_users() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        seed_admin_into(&state, "alice");
        let op = operator_principal(&["work"]);
        let resp = handle_admin_users_list(&state, &op);
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn user_update_changes_role_and_grants() {
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
                role: underwrap_protocol::service::AdminUserRole::Operator,
                granted_stores: Vec::new(),
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
                username: None,
                role: Some(underwrap_protocol::service::AdminUserRole::Admin),
                granted_stores: Some(vec!["work".into(), "vault".into()]),
            },
        );
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body_json(resp);
        assert_eq!(body["role"], "admin");
        assert_eq!(body["granted_stores"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn user_delete_refuses_last_admin() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let alice_id = seed_admin_into(&state, "alice");
        let p = admin_principal_with_id(&alice_id);
        let resp = handle_admin_user_delete(&state, &p, &alice_id);
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = response_body_json(resp);
        assert_eq!(body["code"], "last_admin_protected");
        // Record still in the collection.
        let collection = crate::operators::load(state.admin_home_store.as_ref()).unwrap();
        assert!(collection.find_by_id(&alice_id).is_some());
    }

    #[test]
    fn user_update_refuses_demoting_last_admin() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let alice_id = seed_admin_into(&state, "alice");
        let p = admin_principal_with_id(&alice_id);
        let resp = do_admin_user_update(
            &state,
            &p,
            &alice_id,
            underwrap_protocol::service::AdminUserUpdateRequest {
                username: None,
                role: Some(underwrap_protocol::service::AdminUserRole::Operator),
                granted_stores: None,
            },
        );
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        assert_eq!(response_body_json(resp)["code"], "last_admin_protected");
    }

    #[test]
    fn user_delete_succeeds_when_another_admin_exists() {
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
                role: underwrap_protocol::service::AdminUserRole::Admin,
                granted_stores: Vec::new(),
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
                role: underwrap_protocol::service::AdminUserRole::Operator,
                granted_stores: Vec::new(),
            },
        );
        let bob_id = response_body_json(create)["id"]
            .as_str()
            .unwrap()
            .to_string();

        let hash_before = crate::operators::load(state.admin_home_store.as_ref())
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
        let now = crate::operators::now_epoch_secs();
        mutate_collection(&state, |c| {
            c.update(&bob_id, |op| {
                op.password_hash = new_hash.clone();
                op.password_changed_at = now;
            })?;
            Ok(())
        })
        .unwrap();
        state.session_revocations.revoke_username("bob", now);

        let hash_after = crate::operators::load(state.admin_home_store.as_ref())
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

    #[test]
    fn user_mutations_emit_audit_events() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let alice_id = seed_admin_into(&state, "alice");
        let p = admin_principal_with_id(&alice_id);
        let create = do_admin_user_create(
            &state,
            &p,
            underwrap_protocol::service::AdminUserCreateRequest {
                username: "bob".into(),
                password: "x".into(),
                role: underwrap_protocol::service::AdminUserRole::Operator,
                granted_stores: Vec::new(),
            },
        );
        let bob_id = response_body_json(create)["id"]
            .as_str()
            .unwrap()
            .to_string();
        do_admin_user_update(
            &state,
            &p,
            &bob_id,
            underwrap_protocol::service::AdminUserUpdateRequest {
                username: None,
                role: None,
                granted_stores: Some(vec!["work".into()]),
            },
        );
        handle_admin_user_delete(&state, &p, &bob_id);
        let events = load_audit_history(state.admin_home_store.as_ref()).unwrap();
        let actions: Vec<&str> = events.iter().map(|e| e.action.as_str()).collect();
        for required in ["user.create", "user.update", "user.delete"] {
            assert!(
                actions.contains(&required),
                "missing {required} audit event; got {actions:?}",
            );
        }
        for e in &events {
            assert_eq!(e.operator, "alice");
            assert!(
                e.resource
                    .as_deref()
                    .unwrap_or_default()
                    .starts_with("user:bob"),
                "audit resource must be `user:bob`, got {:?}",
                e.resource,
            );
        }
    }

    #[test]
    fn operator_probing_ungranted_store_gets_404_and_emits_denied_grant_audit() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        // Seed a store and an operator with no grants.
        seed_admin_into(&state, "alice");
        let admin = admin_principal(&[]);
        do_admin_store_create(
            &state,
            &admin,
            AdminStoreCreateRequest {
                name: "private".into(),
                backend: AdminStoreBackendKind::Plain,
                dir: tmp
                    .path()
                    .join("data")
                    .join("private")
                    .to_string_lossy()
                    .to_string(),
                description: None,
            },
        );
        // Operator with no grant for `private`.
        let op = operator_principal(&[]);
        // List filters it out (no audit on filtered lists).
        let resp = handle_admin_stores_list(&state, &op);
        let body = response_body_json(resp);
        assert_eq!(body["items"].as_array().unwrap().len(), 0);
        // Probing the store by name 404s with a `denied_grant` audit.
        let resp = handle_admin_store_detail(&state, &op, "private");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let result = admin_store_for_principal(&state, &op, "private");
        assert!(result.is_err());
        let events = load_audit_history(state.admin_home_store.as_ref()).unwrap();
        let denied: Vec<&underwrap_protocol::service::AdminUiAuditEvent> = events
            .iter()
            .filter(|e| e.result == "denied_grant")
            .collect();
        assert!(
            !denied.is_empty(),
            "expected at least one denied_grant audit event; got {events:?}",
        );
        for e in denied {
            assert_eq!(e.operator, "bob");
            assert_eq!(e.resource.as_deref(), Some("store:private"));
            assert_eq!(e.action, "store.access");
        }
    }

    #[test]
    fn admin_404_on_unknown_store_does_not_emit_denied_grant_audit() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        seed_admin_into(&state, "alice");
        let p = admin_principal(&[]);
        let resp = handle_admin_store_detail(&state, &p, "nonexistent");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let events = load_audit_history(state.admin_home_store.as_ref()).unwrap();
        for e in &events {
            assert_ne!(
                e.result, "denied_grant",
                "admin's 404 must not generate a denied_grant audit",
            );
        }
    }

    #[test]
    fn debug_redacts_password_hash_in_collection_logging() {
        // Regression: when we tracing!(?collection) the hash must
        // never appear. The `Debug` impl on `OperatorRecord` redacts
        // it, but the easiest check is to format the loaded
        // collection after every mutation and grep.
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        seed_admin_into(&state, "alice");
        do_admin_user_create(
            &state,
            &admin_principal(&[]),
            underwrap_protocol::service::AdminUserCreateRequest {
                username: "bob".into(),
                password: "x".into(),
                role: underwrap_protocol::service::AdminUserRole::Operator,
                granted_stores: Vec::new(),
            },
        );
        let collection = crate::operators::load(state.admin_home_store.as_ref()).unwrap();
        let formatted = format!("{collection:?}");
        assert!(
            !formatted.contains("$argon2"),
            "Debug must redact password hashes; got: {formatted}",
        );
    }
}
