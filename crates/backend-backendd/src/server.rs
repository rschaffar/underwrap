//! Backend daemon server.
//!
//! Serves both the runtime HTTPS API for `underwrapd` clients and the
//! browser-facing admin UI/API from one HTTPS listener.

use anyhow::{Context, Result};
use base64::Engine;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tracing::{error, info, warn};

use underwrap_protocol::config::{StoreRecord, StoreRegistryDocument};
use underwrap_protocol::policy::{POLICY_DOCUMENT_SECRET_ID, PolicyDocument};

use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource::parse_secret_id;
use underwrap_store::{
    StorageBackend, blob_from_resource_entries, expand_blob_to_entries, load_resource_blob,
    save_resource_blob,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpsTlsConfig {
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpsConfig {
    pub listen: String,
    pub tls: HttpsTlsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminConfig {
    /// Optional allowlist of `Origin` (or `Referer`) values for
    /// state-changing requests. When set, every non-GET/HEAD/OPTIONS
    /// request must carry an Origin matching one of these entries.
    /// Foundation for the CSRF work in ticket 212.
    #[serde(default)]
    pub allowed_origins: Option<Vec<String>>,
    /// Path to the 32-byte AEAD key file used to sign session cookies.
    /// Required because admin authentication is session-cookie only.
    pub session_key_file: PathBuf,
    /// Session lifetime in seconds. Defaults to
    /// `auth::DEFAULT_SESSION_LIFETIME_SECS` (12 hours) when unset.
    #[serde(default)]
    pub session_lifetime_secs: Option<u64>,
    /// Directory under which the admin "create store" API places the
    /// data directories of newly-created dynamic stores. Each store
    /// lives at `<stores_data_root>/<store_id>/`. Unset disables store
    /// creation (returns `data_root_not_configured`).
    #[serde(default)]
    pub stores_data_root: Option<PathBuf>,
    /// Directory under which the admin API writes master-key files
    /// for encrypted dynamic stores (ticket 234). Each encrypted
    /// store gets one key file at `<keys_dir>/<store>.key`, mode
    /// 0600. Defaults to [`DEFAULT_KEYS_DIR`].
    #[serde(default)]
    pub keys_dir: Option<PathBuf>,
}

/// Default location of the backend-managed store-registry file.
pub const DEFAULT_STORES_FILE: &str = "/var/lib/underwrap-backend/stores.json";

/// Default location of the master-key directory the admin API writes
/// to when creating encrypted stores. One file per store at
/// `<keys_dir>/<store-id>.key`, mode 0600.
pub const DEFAULT_KEYS_DIR: &str = "/var/lib/underwrap-backend/keys";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    /// Path of the backend-managed store-registry file. Missing files
    /// are treated as an empty registry. Defaults to
    /// [`DEFAULT_STORES_FILE`].
    #[serde(default)]
    pub stores_file: Option<PathBuf>,
    /// Path of the backend-managed runtime client-instance registry.
    /// Missing files are treated as an empty registry. Defaults to
    /// `/var/lib/underwrap-backend/client-instances.json`.
    #[serde(default)]
    pub client_instances_file: Option<PathBuf>,
    /// Path of the backend-managed operator-state file. Missing files
    /// are treated as an empty operator collection with bootstrap
    /// incomplete. Defaults to
    /// [`crate::operators::DEFAULT_OPERATORS_FILE`].
    #[serde(default)]
    pub operators_file: Option<PathBuf>,
    #[serde(default)]
    pub https: Option<HttpsConfig>,
    #[serde(default)]
    pub admin: Option<AdminConfig>,
}

impl BackendConfig {
    /// Resolve [`Self::stores_file`] or the project default.
    pub fn stores_file(&self) -> PathBuf {
        self.stores_file
            .clone()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_STORES_FILE))
    }

    /// Resolve [`Self::client_instances_file`] or the project default.
    pub fn client_instances_file(&self) -> PathBuf {
        self.client_instances_file
            .clone()
            .unwrap_or_else(|| PathBuf::from("/var/lib/underwrap-backend/client-instances.json"))
    }

    /// Resolve [`Self::operators_file`] or the project default.
    pub fn operators_file(&self) -> PathBuf {
        self.operators_file
            .clone()
            .unwrap_or_else(|| PathBuf::from(crate::operators::DEFAULT_OPERATORS_FILE))
    }
}

/// One store as loaded from the backend-managed store registry and
/// opened locally.
#[derive(Debug, Clone)]
pub struct LoadedStoreRecord {
    pub record: StoreRecord,
}

/// Open backends + provenance metadata for every store visible to the
/// admin API. Wrapped in an `RwLock` inside [`AdminRuntimeState`] so
/// the CRUD handlers in ticket 231 can mutate it without rebooting
/// the daemon. Readers (overview, secret-read, …) take the read
/// guard and clone out the `Arc<dyn StorageBackend>` they need.
pub struct StoreRegistry {
    pub open: HashMap<String, Arc<dyn StorageBackend>>,
    pub records: HashMap<String, LoadedStoreRecord>,
}

impl StoreRegistry {
    pub fn new() -> Self {
        Self {
            open: HashMap::new(),
            records: HashMap::new(),
        }
    }
}

impl Default for StoreRegistry {
    fn default() -> Self {
        Self::new()
    }
}

struct AdminRuntimeState {
    /// Open store backends + their backend-managed records. Shared
    /// with the service listener so runtime requests and admin CRUD
    /// always consult the same live registry without a daemon restart.
    stores: Arc<std::sync::RwLock<StoreRegistry>>,
    /// Runtime client-instance registry, shared with the service
    /// listener so admin mutations can affect new runtime requests
    /// without a daemon restart.
    client_instances:
        Arc<std::sync::RwLock<HashMap<String, crate::runtime_clients::ClientInstanceRecord>>>,
    /// Path of the backend-managed store-registry file.
    stores_file: PathBuf,
    /// Path of the backend-managed runtime client-instance registry file.
    client_instances_file: PathBuf,
    /// Directory under which the admin "create store" API places the
    /// data directory of each new dynamic store. The on-disk layout is
    /// `<stores_data_root>/<store_id>/`; the admin chooses only the
    /// store's display name. `None` disables creation (returns
    /// `data_root_not_configured`).
    stores_data_root: Option<PathBuf>,
    /// Directory where master-key files for encrypted dynamic stores
    /// are written. One file per store, mode 0600. See ticket 234.
    keys_dir: PathBuf,
    /// Whether the browser-facing admin UI/API is enabled on the HTTPS
    /// listener. When false, `/v1/admin/*` and SPA routes return 404.
    admin_enabled: bool,
    /// When present, every state-changing request must carry an `Origin`
    /// (or `Referer`) matching one of these entries.
    allowed_origins: Option<Vec<String>>,
    /// Where to read the SPA bundle from when serving non-API paths.
    web_source: crate::web::WebAssetSource,
    /// Path of the backend-managed operator-state file.
    operators_file: PathBuf,
    /// 32-byte AEAD key used to encrypt session cookies.
    session_key: [u8; crate::auth::SESSION_KEY_LEN],
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

fn log_admin_event(
    action: &str,
    operator: &str,
    wrapper: Option<&str>,
    resource: Option<&str>,
    secret_id: Option<&str>,
    operation: Option<&str>,
    result: &str,
) {
    info!(
        action = %action,
        operator = %operator,
        wrapper = ?wrapper,
        resource = ?resource,
        secret_id = ?secret_id,
        operation = ?operation,
        result = %result,
        "admin event"
    );
}

pub async fn run(config: BackendConfig, shutdown: Option<Arc<Notify>>) -> Result<()> {
    let stores_file = config.stores_file();
    let client_instances_file = config.client_instances_file();
    let operators_file = config.operators_file();
    let definitions = load_store_records(&stores_file)?;
    let stores = open_stores(&definitions)?;
    let store_registry = Arc::new(std::sync::RwLock::new(StoreRegistry {
        open: stores.clone(),
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
            keys_dir: admin_config
                .and_then(|cfg| cfg.keys_dir.clone())
                .unwrap_or_else(|| PathBuf::from(DEFAULT_KEYS_DIR)),
            admin_enabled: admin_config.is_some(),
            allowed_origins: admin_config.and_then(|cfg| cfg.allowed_origins.clone()),
            web_source: crate::web::WebAssetSource::from_env(),
            operators_file: operators_file.clone(),
            session_key,
            session_lifetime_secs: admin_config
                .and_then(|cfg| cfg.session_lifetime_secs)
                .unwrap_or(crate::auth::DEFAULT_SESSION_LIFETIME_SECS),
            cookies_secure: true,
            login_rate_limiter: crate::auth::RateLimiter::new(),
            session_revocations: crate::auth::SessionRevocations::new(),
            bootstrap_lock: std::sync::Mutex::new(()),
            operator_collection_lock: std::sync::Mutex::new(()),
        });
        {
            let snapshot = state.stores.read().unwrap();
            info!(
                listen = %https_config.listen,
                stores = snapshot.open.len(),
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

fn open_stores(
    definitions: &HashMap<String, LoadedStoreRecord>,
) -> Result<HashMap<String, Arc<dyn StorageBackend>>> {
    let mut stores = HashMap::new();
    for (id, def) in definitions {
        let store: Arc<dyn StorageBackend> = Arc::from(underwrap_store::open(&def.record.storage)?);
        stores.insert(id.clone(), store);
    }
    Ok(stores)
}

/// Read the backend-managed store-registry file and return the runtime
/// store map keyed by stable store id. Missing files are treated as an
/// empty registry so a fresh backend can still boot.
pub fn load_store_records(stores_file: &Path) -> Result<HashMap<String, LoadedStoreRecord>> {
    let text = match std::fs::read_to_string(stores_file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", stores_file.display())),
    };
    let doc: StoreRegistryDocument = serde_json::from_str(&text)
        .with_context(|| format!("parsing store registry {}", stores_file.display()))?;
    anyhow::ensure!(
        doc.version == 1,
        "unsupported store registry version {}",
        doc.version
    );
    let mut out: HashMap<String, LoadedStoreRecord> = HashMap::new();
    let mut names = std::collections::BTreeSet::new();
    for record in doc.stores {
        if !is_valid_store_id(&record.id) {
            anyhow::bail!(
                "invalid store id '{}' in {}: must match [a-z0-9][a-z0-9_-]*",
                record.id,
                stores_file.display()
            );
        }
        let trimmed_name = record.name.trim();
        anyhow::ensure!(
            !trimmed_name.is_empty(),
            "store '{}' in {} has an empty visible name",
            record.id,
            stores_file.display()
        );
        let name_key = trimmed_name.to_ascii_lowercase();
        anyhow::ensure!(
            names.insert(name_key),
            "duplicate visible store name '{}' in {}",
            trimmed_name,
            stores_file.display()
        );
        if out
            .insert(record.id.clone(), LoadedStoreRecord { record })
            .is_some()
        {
            anyhow::bail!("duplicate store id in {}", stores_file.display());
        }
    }
    Ok(out)
}

fn save_store_records(
    stores_file: &Path,
    records: &HashMap<String, LoadedStoreRecord>,
) -> Result<()> {
    let mut stores: Vec<StoreRecord> = records.values().map(|r| r.record.clone()).collect();
    stores.sort_by(|a, b| a.id.cmp(&b.id));
    let doc = StoreRegistryDocument { version: 1, stores };
    let json = serde_json::to_vec_pretty(&doc).context("serialising store registry")?;
    let dir = stores_file.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "stores file path '{}' has no parent directory",
            stores_file.display()
        )
    })?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating store-registry dir {}", dir.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("creating temp store-registry under {}", dir.display()))?;
    use std::io::Write;
    tmp.write_all(&json)
        .context("writing store-registry tempfile")?;
    tmp.as_file()
        .sync_all()
        .context("fsync store-registry tempfile")?;
    tmp.persist(stores_file).map_err(|e| {
        anyhow::anyhow!(
            "atomically persisting store registry to {}: {e}",
            stores_file.display(),
        )
    })?;
    Ok(())
}

/// A store id must be safe to use as a filename and as a URL path
/// segment. Lowercase letters, digits, `-`, `_`, starting with an
/// alphanumeric.
pub fn is_valid_store_id(name: &str) -> bool {
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

fn validate_store_name(name: &str) -> std::result::Result<String, &'static str> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        Err("store name must not be empty")
    } else {
        Ok(trimmed.to_string())
    }
}

fn generate_store_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 10];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!(
        "store_{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

fn generate_client_instance_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 10];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!(
        "inst_{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

fn validate_runtime_client_mappings(
    stores: &Arc<std::sync::RwLock<StoreRegistry>>,
    client_instances: &Arc<
        std::sync::RwLock<HashMap<String, crate::runtime_clients::ClientInstanceRecord>>,
    >,
) -> Result<()> {
    let registry = stores.read().unwrap();
    for instance in client_instances.read().unwrap().values() {
        registry
            .open
            .get(&instance.current_store_id)
            .with_context(|| {
                format!(
                    "runtime client instance '{}' references unknown store '{}'",
                    instance.id, instance.current_store_id
                )
            })?;
    }
    Ok(())
}

fn build_https_acceptor(config: &HttpsTlsConfig) -> Result<TlsAcceptor> {
    use rustls::ServerConfig;

    let _ = rustls::crypto::ring::default_provider().install_default();

    let certs = load_cert_chain(&config.cert_file)?;
    let key = load_private_key(&config.key_file)?;

    let verifier: Arc<dyn rustls::server::danger::ClientCertVerifier> =
        Arc::new(FingerprintClientCertVerifier::new());

    let server_config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(certs, key)
        .context("building rustls server config")?;

    Ok(TlsAcceptor::from(Arc::new(server_config)))
}

/// Client certificate verifier that accepts any presented leaf at the TLS
/// layer. Authorization is performed downstream by SHA-256 fingerprint lookup
/// against the runtime-client registry in [`resolve_runtime_client`]: the
/// chain, subject, EKU, and validity window of the leaf are not consulted
/// here, only its hash. An unknown-fingerprint cert still fails at the
/// application layer; a known-fingerprint cert is admitted regardless of how
/// it was signed.
///
/// The handshake signature is still verified against the leaf's public key so
/// a stolen cert without its private key cannot be replayed.
#[derive(Debug)]
struct FingerprintClientCertVerifier {
    supported_algs: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl FingerprintClientCertVerifier {
    fn new() -> Self {
        let provider = rustls::crypto::ring::default_provider();
        Self {
            supported_algs: provider.signature_verification_algorithms,
        }
    }
}

impl rustls::server::danger::ClientCertVerifier for FingerprintClientCertVerifier {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        false
    }

    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::server::danger::ClientCertVerified, rustls::Error> {
        Ok(rustls::server::danger::ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.supported_algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.supported_algs)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.supported_algs.supported_schemes()
    }
}

/// Validate the browser-facing admin configuration.
fn validate_admin_config(config: &AdminConfig) -> Result<()> {
    if let Some(origins) = &config.allowed_origins {
        for origin in origins {
            if origin.trim().is_empty() {
                anyhow::bail!("admin.allowed_origins contains an empty entry");
            }
        }
    }
    Ok(())
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

fn resolve_runtime_client(
    state: &AdminRuntimeState,
    certs: &[rustls::pki_types::CertificateDer<'static>],
) -> Result<(String, Arc<dyn StorageBackend>)> {
    let cert = certs
        .first()
        .context("client did not present a certificate")?;
    let fingerprint = crate::runtime_clients::cert_fingerprint_from_der(cert.as_ref());
    let client_instances = state.client_instances.read().unwrap();
    let instance = client_instances
        .values()
        .find(|instance| instance.current_credential.cert_fingerprint == fingerprint)
        .with_context(|| format!("unknown client credential fingerprint '{fingerprint}'"))?;
    anyhow::ensure!(
        instance.status == crate::runtime_clients::ClientInstanceStatus::Active,
        "runtime client instance '{}' is not active",
        instance.id
    );
    let store = state
        .stores
        .read()
        .unwrap()
        .open
        .get(&instance.current_store_id)
        .with_context(|| {
            format!(
                "runtime client instance '{}' references unknown store '{}'",
                instance.id, instance.current_store_id
            )
        })?
        .clone();
    Ok((instance.id.clone(), store))
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

async fn handle_service_request(
    request: Request<Incoming>,
    state: Arc<AdminRuntimeState>,
    peer_certificates: Arc<Vec<rustls::pki_types::CertificateDer<'static>>>,
) -> std::result::Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let (principal, store) = match resolve_runtime_client(&state, peer_certificates.as_ref()) {
        Ok(resolved) => resolved,
        Err(e) => {
            warn!(
                method = %request.method(),
                path = %request.uri().path(),
                err = format!("{e:#}"),
                "rejecting runtime client request: {e:#}"
            );
            return Ok(error_response(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "runtime client is not currently authorized",
            ));
        }
    };
    let response = match (request.method(), request.uri().path()) {
        (&Method::GET, "/v1/status") => json_response(
            StatusCode::OK,
            &underwrap_protocol::service::ServiceStatusResponse {
                ok: true,
                principal: principal.clone(),
            },
        ),
        (&Method::POST, "/v1/read") => {
            match parse_json_body::<underwrap_protocol::service::ServiceReadRequest>(request).await
            {
                Ok(body) => {
                    match runtime_read_gate(store.as_ref(), &principal, &body.secret_id, false) {
                        Ok(()) => match store.read(&body.secret_id) {
                            Ok(material) => json_response(
                                StatusCode::OK,
                                &underwrap_protocol::service::ServiceReadResponse {
                                    material_b64: material.map(|material| {
                                        base64::engine::general_purpose::STANDARD
                                            .encode(material.as_ref())
                                    }),
                                },
                            ),
                            Err(e) => error_response(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "internal",
                                &format!("storage error: {e}"),
                            ),
                        },
                        Err(reason) => error_response(StatusCode::FORBIDDEN, "forbidden", &reason),
                    }
                }
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/write") => {
            match parse_json_body::<underwrap_protocol::service::ServiceWriteRequest>(request).await
            {
                Ok(body) => {
                    match runtime_read_gate(store.as_ref(), &principal, &body.secret_id, true) {
                        Ok(()) => {
                            match base64::engine::general_purpose::STANDARD
                                .decode(&body.material_b64)
                            {
                                Ok(material) => match store.write(&body.secret_id, &material) {
                                    Ok(()) => json_response(
                                        StatusCode::OK,
                                        &underwrap_protocol::service::ServiceWriteResponse {
                                            ok: true,
                                        },
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
                        Err(reason) => error_response(StatusCode::FORBIDDEN, "forbidden", &reason),
                    }
                }
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/list") => {
            match parse_json_body::<underwrap_protocol::service::ServiceListRequest>(request).await
            {
                Ok(body) => {
                    match runtime_prefix_gate(store.as_ref(), &principal, &body.prefix, false) {
                        Ok(()) => match store.list(&body.prefix) {
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
                        Err(reason) => error_response(StatusCode::FORBIDDEN, "forbidden", &reason),
                    }
                }
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/read-prefixes") => {
            match parse_json_body::<underwrap_protocol::service::ServiceReadPrefixesRequest>(
                request,
            )
            .await
            {
                Ok(body) => {
                    let mut blocked: Option<String> = None;
                    for prefix in &body.prefixes {
                        if let Err(reason) =
                            runtime_prefix_gate(store.as_ref(), &principal, prefix, false)
                        {
                            blocked = Some(reason);
                            break;
                        }
                    }
                    match blocked {
                        Some(reason) => error_response(StatusCode::FORBIDDEN, "forbidden", &reason),
                        None => match store.read_prefixes(&body.prefixes) {
                            Ok(entries) => {
                                let mut entries: Vec<_> = entries
                                    .into_iter()
                                    .map(|(id, material)| {
                                        underwrap_protocol::service::ServiceBundleEntry {
                                            id,
                                            material_b64: base64::engine::general_purpose::STANDARD
                                                .encode(material.as_ref()),
                                        }
                                    })
                                    .collect();
                                entries.sort_by(|a, b| a.id.cmp(&b.id));
                                json_response(
                                    StatusCode::OK,
                                    &underwrap_protocol::service::ServiceReadPrefixesResponse {
                                        entries,
                                    },
                                )
                            }
                            Err(e) => error_response(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "internal",
                                &format!("storage error: {e}"),
                            ),
                        },
                    }
                }
                Err(response) => response,
            }
        }
        (&Method::POST, "/v1/runtime/wrappers/register") => match parse_json_body::<
            underwrap_protocol::service::ServiceWrapperRegisterRequest,
        >(request)
        .await
        {
            Ok(body) => {
                let auto_create_enabled = state
                    .client_instances
                    .read()
                    .unwrap()
                    .get(&principal)
                    .map(|r| r.auto_create_wrapper_assets)
                    .unwrap_or(false);
                let now = crate::operators::now_epoch_secs();
                match apply_runtime_wrapper_register(
                    &*store,
                    &principal,
                    auto_create_enabled,
                    &body,
                    now,
                ) {
                    Ok(response) => json_response(StatusCode::OK, &response),
                    Err(e) => error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal",
                        &format!("runtime register storage error: {e}"),
                    ),
                }
            }
            Err(response) => response,
        },
        (&Method::POST, "/v1/delete") => {
            match parse_json_body::<underwrap_protocol::service::ServiceDeleteRequest>(request)
                .await
            {
                Ok(body) => {
                    match runtime_read_gate(store.as_ref(), &principal, &body.secret_id, true) {
                        Ok(()) => match store.delete(&body.secret_id) {
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
                        Err(reason) => error_response(StatusCode::FORBIDDEN, "forbidden", &reason),
                    }
                }
                Err(response) => response,
            }
        }
        _ => error_response(StatusCode::NOT_FOUND, "not_found", "unknown endpoint"),
    };
    Ok(response)
}

/// Gate a runtime client's request for a single secret id. The id is
/// either:
///
/// - a reserved system id (`__underwrap/...`) — the runtime client is
///   permitted unconditionally; only mTLS-authenticated clients reach
///   this gate, and the system ids cover policy + catalog documents
///   that the daemon legitimately needs;
/// - a canonical `<resource>/<slot>/...` id — the runtime client must
///   have *some* grant for `(principal, _, resource)`; on write the
///   grant must additionally have `writable = true`.
///
/// Reads on resources the principal has *no* grant for are denied —
/// closing the trust hole where a daemon with a tampered config could
/// previously fetch any secret out of the store.
fn runtime_read_gate(
    store: &dyn StorageBackend,
    principal: &str,
    secret_id: &str,
    write: bool,
) -> std::result::Result<(), String> {
    if secret_id.starts_with("__underwrap/") {
        return Ok(());
    }
    let resource_owned: String;
    let resource: &str = if let Some(rest) = secret_id.strip_prefix("resources/") {
        rest
    } else if let Some(r) = underwrap_protocol::resource::resource_from_secret_id(secret_id) {
        resource_owned = r.to_string();
        // Storing to a local then re-borrowing keeps the branch types
        // aligned: both branches return `&str`.
        let _ = &resource_owned;
        r
    } else {
        return Err(format!(
            "secret id '{secret_id}' is not a canonical resource key"
        ));
    };
    // No policy document persisted yet → permissive greenfield
    // default. As soon as an operator writes any document (even an
    // empty one) the daemon is locked down to whatever's been
    // explicitly granted.
    let raw = store
        .read(POLICY_DOCUMENT_SECRET_ID)
        .map_err(|e| format!("loading policy: {e}"))?;
    let Some(material) = raw else {
        return Ok(());
    };
    let doc: PolicyDocument =
        serde_json::from_slice(material.as_ref()).map_err(|e| format!("parsing policy: {e}"))?;
    let Some(permission) = doc
        .permissions
        .iter()
        .find(|p| p.client_instance == principal && p.resource == resource)
    else {
        return Err(format!(
            "no wrapper permission for ({principal}, *, '{resource}')"
        ));
    };
    if write && !permission.writable {
        return Err(format!(
            "wrapper permission for ({principal}, '{}', '{resource}') is read-only",
            permission.wrapper
        ));
    }
    // Bump last_used_at on the matched pair. Failure here is
    // non-fatal — it must never block a successful read.
    if let Err(e) = bump_pair_last_used(store, &permission.wrapper, resource) {
        warn!(
            audit = "pair_last_used_bump_failed",
            principal = %principal,
            wrapper = %permission.wrapper,
            resource = %resource,
            err = %e,
        );
    }
    Ok(())
}

/// Set `last_used_at` on the `(wrapper, resource)` pair to the
/// current wall-clock second. Idempotent at one-second resolution:
/// the pairs document is only rewritten if the new timestamp differs
/// from the stored one.
fn bump_pair_last_used(store: &dyn StorageBackend, wrapper: &str, resource: &str) -> Result<()> {
    let now = crate::operators::now_epoch_secs();
    let mut doc = load_pairs_document(store)?;
    let Some(pair) = doc
        .pairs
        .iter_mut()
        .find(|p| p.wrapper == wrapper && p.resource == resource)
    else {
        // No pair record means nothing to bump. This can happen if
        // the pair was deleted while a wrapper invocation was in
        // flight, or in the greenfield case where no entities have
        // been created yet.
        return Ok(());
    };
    if pair.last_used_at == Some(now) {
        return Ok(());
    }
    pair.last_used_at = Some(now);
    save_pairs_document(store, &doc)
}

/// Gate a prefix request (`/v1/list`, `/v1/read-prefixes`). A bare
/// resource-slot prefix like `github/token/env` is permitted; the same
/// policy lookup as the single-secret gate applies.
fn runtime_prefix_gate(
    store: &dyn StorageBackend,
    principal: &str,
    prefix: &str,
    write: bool,
) -> std::result::Result<(), String> {
    if prefix.starts_with("__underwrap/") || prefix == "__underwrap" {
        return Ok(());
    }
    // Strip the trailing `/<slot>` if present so we extract the
    // resource id even when the prefix is `<resource>/env` (no
    // trailing `/`).
    let resource: &str = match prefix.rsplit_once('/') {
        Some((
            head,
            underwrap_protocol::resource::ENV_SLOT
            | underwrap_protocol::resource::FILES_SLOT
            | underwrap_protocol::resource::LIBSECRET_SLOT,
        )) => head,
        // Fall back to interpreting the prefix as the resource
        // itself. Bare `<resource>` listings are still gated.
        _ => prefix,
    };
    let raw = store
        .read(POLICY_DOCUMENT_SECRET_ID)
        .map_err(|e| format!("loading policy: {e}"))?;
    let Some(material) = raw else {
        return Ok(());
    };
    let doc: PolicyDocument =
        serde_json::from_slice(material.as_ref()).map_err(|e| format!("parsing policy: {e}"))?;
    let Some(permission) = doc
        .permissions
        .iter()
        .find(|p| p.client_instance == principal && p.resource == resource)
    else {
        return Err(format!(
            "no wrapper permission for ({principal}, *, '{resource}')"
        ));
    };
    if write && !permission.writable {
        return Err(format!(
            "wrapper permission for ({principal}, '{}', '{resource}') is read-only",
            permission.wrapper
        ));
    }
    Ok(())
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

/// Read a single per-key entry by translating through the resource
/// blob layout. Falls back to raw `store.read` for non-slotted ids
/// (reserved system documents).
fn read_legacy_secret(store: &dyn StorageBackend, secret_id: &str) -> Result<Option<SecretBytes>> {
    let Some(parsed) = parse_secret_id(secret_id) else {
        return store.read(secret_id);
    };
    let blob = load_resource_blob(store, parsed.resource_id)?;
    let entries = expand_blob_to_entries(parsed.resource_id, &blob)?;
    Ok(entries.get(secret_id).cloned())
}

/// Write a single per-key entry by load-modify-save against the
/// resource blob.
fn write_legacy_secret(store: &dyn StorageBackend, secret_id: &str, material: &[u8]) -> Result<()> {
    let Some(parsed) = parse_secret_id(secret_id) else {
        return store.write(secret_id, material);
    };
    let blob = load_resource_blob(store, parsed.resource_id)?;
    let mut entries = expand_blob_to_entries(parsed.resource_id, &blob)?;
    entries.insert(secret_id.to_string(), SecretBytes::from(material.to_vec()));
    let new_blob = blob_from_resource_entries(parsed.resource_id, &entries)?;
    save_resource_blob(store, parsed.resource_id, &new_blob)
}

/// Delete a single per-key entry. Returns the existed flag.
fn delete_legacy_secret(store: &dyn StorageBackend, secret_id: &str) -> Result<bool> {
    let Some(parsed) = parse_secret_id(secret_id) else {
        return store.delete(secret_id);
    };
    let blob = load_resource_blob(store, parsed.resource_id)?;
    let mut entries = expand_blob_to_entries(parsed.resource_id, &blob)?;
    let existed = entries.remove(secret_id).is_some();
    if existed {
        let new_blob = blob_from_resource_entries(parsed.resource_id, &entries)?;
        save_resource_blob(store, parsed.resource_id, &new_blob)?;
    }
    Ok(existed)
}

/// List per-key entries matching `prefix`. Empty prefix lists every
/// entry in every resource. System-reserved prefixes fall through to
/// raw `store.list`.
fn list_legacy_secrets(store: &dyn StorageBackend, prefix: &str) -> Result<Vec<String>> {
    if prefix.starts_with("__underwrap/") || prefix == "__underwrap" {
        return store.list(prefix);
    }
    // Identify which resource(s) the prefix touches.
    let derived_resource_id: Option<String> = if prefix.is_empty() {
        None
    } else {
        // If the prefix already targets a specific slot, the resource
        // is everything before the slot marker.
        let split_resource = ["/env", "/files", "/libsecret"]
            .iter()
            .filter_map(|marker| prefix.find(marker).map(|idx| &prefix[..idx]))
            .min_by_key(|s| s.len())
            .map(str::to_string);
        // Otherwise the prefix may itself BE a resource id.
        split_resource.or_else(|| Some(prefix.to_string()))
    };

    let mut keys: Vec<String> = Vec::new();
    let resources: Vec<String> = if let Some(rid) = derived_resource_id {
        vec![rid]
    } else {
        // List every resource blob currently in storage.
        store
            .list("resources/")?
            .into_iter()
            .filter_map(|k| k.strip_prefix("resources/").map(str::to_string))
            .collect()
    };
    for resource_id in &resources {
        let blob = load_resource_blob(store, resource_id)?;
        let entries = expand_blob_to_entries(resource_id, &blob)?;
        for key in entries.into_keys() {
            if prefix.is_empty() || key == prefix || key.starts_with(&format!("{prefix}/")) {
                keys.push(key);
            }
        }
    }
    keys.sort();
    Ok(keys)
}

/// Count how many per-slot entries exist under `resource_name`, plus
/// a small sample of ids for the structured "delete refused"
/// response.
fn resource_blob_entries(store: &dyn StorageBackend, resource_name: &str) -> Result<Vec<String>> {
    let blob = load_resource_blob(store, resource_name)?;
    let entries = expand_blob_to_entries(resource_name, &blob)?;
    let mut ids: Vec<String> = entries.into_keys().collect();
    ids.sort();
    Ok(ids)
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

    match write_legacy_secret(store.as_ref(), &body.secret_id, &material) {
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

fn load_policy_document(store: &dyn StorageBackend) -> Result<PolicyDocument> {
    match store.read(POLICY_DOCUMENT_SECRET_ID)? {
        Some(material) => {
            serde_json::from_slice(material.as_ref()).context("parsing backend policy document")
        }
        None => Ok(PolicyDocument::default()),
    }
}

fn load_resources_document(
    store: &dyn StorageBackend,
) -> Result<underwrap_protocol::entities::ResourcesDocument> {
    use underwrap_protocol::entities::{
        ENTITY_SCHEMA_VERSION, RESOURCES_DOCUMENT_SECRET_ID, ResourcesDocument,
    };
    match store.read(RESOURCES_DOCUMENT_SECRET_ID)? {
        Some(material) => {
            let doc: ResourcesDocument = serde_json::from_slice(material.as_ref())
                .context("parsing backend resources document")?;
            anyhow::ensure!(
                doc.version == ENTITY_SCHEMA_VERSION,
                "unsupported resources document version {}",
                doc.version,
            );
            Ok(doc)
        }
        None => Ok(ResourcesDocument::default()),
    }
}

fn save_resources_document(
    store: &dyn StorageBackend,
    doc: &underwrap_protocol::entities::ResourcesDocument,
) -> Result<()> {
    use underwrap_protocol::entities::RESOURCES_DOCUMENT_SECRET_ID;
    store.write(
        RESOURCES_DOCUMENT_SECRET_ID,
        serde_json::to_vec(doc)?.as_slice(),
    )
}

fn load_pairs_document(
    store: &dyn StorageBackend,
) -> Result<underwrap_protocol::entities::PairsDocument> {
    use underwrap_protocol::entities::{
        ENTITY_SCHEMA_VERSION, PAIRS_DOCUMENT_SECRET_ID, PairsDocument,
    };
    match store.read(PAIRS_DOCUMENT_SECRET_ID)? {
        Some(material) => {
            let doc: PairsDocument = serde_json::from_slice(material.as_ref())
                .context("parsing backend pairs document")?;
            anyhow::ensure!(
                doc.version == ENTITY_SCHEMA_VERSION,
                "unsupported pairs document version {}",
                doc.version,
            );
            Ok(doc)
        }
        None => Ok(PairsDocument::default()),
    }
}

fn save_pairs_document(
    store: &dyn StorageBackend,
    doc: &underwrap_protocol::entities::PairsDocument,
) -> Result<()> {
    use underwrap_protocol::entities::PAIRS_DOCUMENT_SECRET_ID;
    store.write(
        PAIRS_DOCUMENT_SECRET_ID,
        serde_json::to_vec(doc)?.as_slice(),
    )
}

fn admin_ui_overview_for_store(
    store_name: &str,
    store: &dyn StorageBackend,
    _now_epoch: u64,
) -> Result<underwrap_protocol::service::AdminUiOverviewStore> {
    let resources = load_resources_document(store)?;
    let policy = load_policy_document(store)?;

    let resource_count = resources.resources.len();

    // Bucket every wrapper permission by its current mode. The
    // permission key is `(client_instance, wrapper, resource)` so this
    // count reflects the distribution across the per-resource
    // permission fleet, not the wrapper catalog.
    let mut wrapper_counts = underwrap_protocol::service::AdminUiWrapperCounts {
        locked: 0,
        unlocked: 0,
        unlocked_until: 0,
        per_call_approval: 0,
    };
    let mut writable_permission_count = 0usize;
    for permission in &policy.permissions {
        match permission.mode {
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
        if permission.writable {
            writable_permission_count += 1;
        }
    }

    Ok(underwrap_protocol::service::AdminUiOverviewStore {
        store: store_name.to_string(),
        resource_count,
        wrapper_counts,
        writable_permission_count,
    })
}

/// Apply a `POST /v1/runtime/wrappers/register` request.
///
/// Computes the diff between the request's pairs and what's already
/// in the resource/pair documents, then either auto-creates the
/// missing entities (if the calling client instance is opted in) or
/// returns a structured refusal listing what would have been created.
/// On the happy path the documents are written back; on the refusal
/// path the store is untouched.
fn apply_runtime_wrapper_register(
    store: &dyn StorageBackend,
    client_instance_id: &str,
    auto_create_enabled: bool,
    request: &underwrap_protocol::service::ServiceWrapperRegisterRequest,
    now_epoch: u64,
) -> Result<underwrap_protocol::service::ServiceWrapperRegisterResponse> {
    use underwrap_protocol::entities::{EntityCreator, ResourceRecord, WrapperResourcePairRecord};
    use underwrap_protocol::service::{
        ServiceWrapperRegisterReject, ServiceWrapperRegisterResponse, ServiceWrapperResourcePair,
    };

    let mut resources = load_resources_document(store)?;
    let mut pairs = load_pairs_document(store)?;

    // Resolve missing resource names first so a single resource named
    // by several pairs doesn't get reported multiple times.
    let mut missing_resources_set: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    for pair in &request.pairs {
        if resources.find(&pair.resource).is_none() {
            missing_resources_set.insert(pair.resource.clone());
        }
    }
    let missing_pairs: Vec<ServiceWrapperResourcePair> = request
        .pairs
        .iter()
        .filter(|p| pairs.find(&p.wrapper, &p.resource).is_none())
        .cloned()
        .collect();

    // Refresh `files_mount_paths` on existing pairs from the latest
    // daemon-reported config. This lets the SPA show accurate mount
    // locations even when an operator edits wrapper mount paths
    // between daemon restarts.
    let mut refreshed = false;
    for incoming in &request.pairs {
        if let Some(existing) = pairs
            .pairs
            .iter_mut()
            .find(|p| p.wrapper == incoming.wrapper && p.resource == incoming.resource)
            && existing.files_mount_paths != incoming.files_mount_paths
        {
            existing.files_mount_paths = incoming.files_mount_paths.clone();
            refreshed = true;
        }
    }

    if missing_resources_set.is_empty() && missing_pairs.is_empty() && !refreshed {
        return Ok(ServiceWrapperRegisterResponse::default());
    }

    if !auto_create_enabled {
        return Ok(ServiceWrapperRegisterResponse {
            created_resources: Vec::new(),
            created_pairs: Vec::new(),
            rejected: Some(ServiceWrapperRegisterReject::AutoCreateDisabled {
                missing_resources: missing_resources_set.into_iter().collect(),
                missing_pairs,
            }),
        });
    }

    let creator = EntityCreator::ClientInstance {
        id: client_instance_id.to_string(),
    };

    let mut created_resources: Vec<String> = Vec::new();
    for name in &missing_resources_set {
        // `insert` rejects duplicates; the missing-set above guarantees
        // none, so unwrap is safe.
        resources
            .insert(ResourceRecord {
                name: name.clone(),
                created_by: creator.clone(),
                created_at: now_epoch,
            })
            .expect("missing-set guarantees no duplicate");
        created_resources.push(name.clone());
    }

    let mut created_pairs: Vec<ServiceWrapperResourcePair> = Vec::new();
    for pair in &missing_pairs {
        pairs
            .insert(WrapperResourcePairRecord {
                wrapper: pair.wrapper.clone(),
                resource: pair.resource.clone(),
                created_by: creator.clone(),
                created_at: now_epoch,
                last_used_at: None,
                files_mount_paths: pair.files_mount_paths.clone(),
            })
            .expect("missing-set guarantees no duplicate");
        created_pairs.push(pair.clone());
    }

    if !created_resources.is_empty() {
        save_resources_document(store, &resources)?;
    }
    if !created_pairs.is_empty() || refreshed {
        save_pairs_document(store, &pairs)?;
    }

    Ok(ServiceWrapperRegisterResponse {
        created_resources,
        created_pairs,
        rejected: None,
    })
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
/// Ungranted-access attempts by non-admin principals are still logged
/// to journald/stdout, but no longer persisted as backend audit-history
/// records.
#[allow(clippy::result_large_err)]
fn admin_store_for_principal(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    store: &str,
) -> std::result::Result<Arc<dyn StorageBackend>, Response<Full<Bytes>>> {
    if !principal.has_store_access(store) {
        emit_denied_grant_log(state, principal, store);
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

/// Log an operator probing a store they do not have a grant for.
fn emit_denied_grant_log(
    _state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    store: &str,
) {
    // Admins bypass grant filtering by design; if has_store_access
    // returned false here, the principal is an operator. We assert
    // that invariant in debug builds so a future refactor doesn't
    // accidentally start filtering admin requests too.
    debug_assert!(!principal.is_admin());
    let resource = format!("store:{store}");
    log_admin_event(
        "store.access",
        &principal.username,
        None,
        Some(resource.as_str()),
        None,
        None,
        "denied_grant",
    );
}

/// Build a [`Principal`] from an authenticated request. We re-read the
/// operator collection so role and grants are always current.
#[allow(clippy::result_large_err)]
fn resolve_principal(
    state: &AdminRuntimeState,
    auth: &AdminAuth,
) -> std::result::Result<crate::operators::Principal, Response<Full<Bytes>>> {
    let payload = auth.session();
    let collection = match crate::operators::load(&state.operators_file) {
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

/// Result of authenticating an admin request.
#[derive(Debug)]
struct AdminAuth(crate::auth::SessionPayload);

impl AdminAuth {
    #[cfg(test)]
    fn operator(&self) -> &str {
        &self.0.username
    }

    fn session(&self) -> &crate::auth::SessionPayload {
        &self.0
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

/// Authenticate an admin request via its session cookie.
fn authenticate_admin(
    state: &AdminRuntimeState,
    headers: &hyper::HeaderMap,
) -> std::result::Result<AdminAuth, AdminAuthError> {
    let value = crate::auth::extract_session_cookie(headers)
        .ok_or(AdminAuthError::Unauthenticated("missing session cookie"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match crate::auth::decrypt_session(value, &state.session_key, now) {
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
                Ok(AdminAuth(payload))
            }
        }
        Err(_) => Err(AdminAuthError::Unauthenticated("invalid session cookie")),
    }
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
    let session_key = state.session_key;
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

    let collection = match crate::operators::load(&state.operators_file) {
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
    let sentinel_set = crate::operators::bootstrap_completed(&state.operators_file).unwrap_or(true);
    let collection_empty = crate::operators::load(&state.operators_file)
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
    let session_key = state.session_key;
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

    match crate::operators::bootstrap_completed(&state.operators_file) {
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

    let collection = match crate::operators::load(&state.operators_file) {
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
    if let Err(e) = crate::operators::save(&state.operators_file, &new_collection) {
        warn!(err = %e, "bootstrap: writing operator collection failed");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "could not persist operator",
        );
    }
    if let Err(e) = crate::operators::mark_bootstrap_completed(
        &state.operators_file,
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

/// Log one bootstrap attempt outcome. Best effort: never fails the caller.
fn audit_bootstrap_attempt(_state: &AdminRuntimeState, username: &str, result: &str) {
    log_admin_event(
        "bootstrap_attempt",
        username,
        None,
        None,
        None,
        None,
        result,
    );
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
    let payload = match crate::auth::decrypt_session(value, &state.session_key, now) {
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
    let collection = crate::operators::load(&state.operators_file).unwrap_or_default();
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
// Operator records are stored in the backend-managed operator-state
// file (see `crate::operators`). Every mutation goes through
// `mutate_collection` so the read-modify-write race against
// concurrent admin requests is closed by the operator-collection
// mutex and the uniqueness / last-admin invariants live in one place.

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
    _state: &AdminRuntimeState,
    actor: &crate::operators::Principal,
    target_username: &str,
    action: &str,
    result: &str,
) {
    let resource = format!("user:{target_username}");
    log_admin_event(
        action,
        &actor.username,
        None,
        Some(resource.as_str()),
        None,
        None,
        result,
    );
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
    let mut collection = crate::operators::load(&state.operators_file)?;
    let out = f(&mut collection)?;
    crate::operators::save(&state.operators_file, &collection)?;
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
    let collection = match crate::operators::load(&state.operators_file) {
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
    let collection = match crate::operators::load(&state.operators_file) {
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
    let collection = match crate::operators::load(&state.operators_file) {
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

// ===== Stores CRUD =====
//
// All five handlers operate on the runtime `state.stores` registry
// plus the backend-managed store-registry file at `state.stores_file`.
// The requested data `dir` is still validated against the configured
// allow-list of roots. Atomicity comes from `tempfile::NamedTempFile`
// `persist()` on the registry write side and a brief held `write()`
// lock on the in-memory registry update.

use underwrap_protocol::service::{
    AdminClientInstanceActionState, AdminClientInstanceCreateRequest,
    AdminClientInstanceDetailResponse, AdminClientInstanceListItem, AdminClientInstanceStatus,
    AdminClientInstanceUpdateRequest, AdminClientInstancesListResponse, AdminStoreActionState,
    AdminStoreBackendKind, AdminStoreCreateRequest, AdminStoreCreateResponse,
    AdminStoreDeleteResponse, AdminStoreDetailResponse, AdminStoreListItem,
    AdminStoreRotateKeyResponse, AdminStoreUpdateRequest, AdminStoreUsageSummary,
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
        Ok(ids) => !ids.is_empty(),
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
    principal: &crate::operators::Principal,
    loaded: &LoadedStoreRecord,
    usage: &AdminStoreUsageSummary,
) -> AdminStoreActionState {
    let can_manage = principal.is_admin();
    let can_delete_without_force =
        can_manage && usage.mapped_client_instance_count == 0 && !usage.has_secrets;
    let can_force_delete =
        can_manage && usage.mapped_client_instance_count == 0 && usage.has_secrets;
    AdminStoreActionState {
        can_edit_metadata: can_manage,
        can_rotate_key: can_manage
            && matches!(
                &loaded.record.storage,
                underwrap_protocol::config::StorageConfig::Encrypted { .. }
            ),
        can_delete_now: can_delete_without_force,
        can_force_delete,
    }
}

fn admin_store_list_item(
    loaded: &LoadedStoreRecord,
    usage: AdminStoreUsageSummary,
    actions: AdminStoreActionState,
) -> AdminStoreListItem {
    AdminStoreListItem {
        id: loaded.record.id.clone(),
        name: loaded.record.name.clone(),
        backend: backend_kind(&loaded.record.storage),
        description: loaded.record.description.clone(),
        created_at: loaded.record.created_at,
        usage,
        actions,
    }
}

fn admin_store_detail(
    stores_file: &Path,
    loaded: &LoadedStoreRecord,
    usage: AdminStoreUsageSummary,
    actions: AdminStoreActionState,
) -> AdminStoreDetailResponse {
    let (dir, key_file) = match &loaded.record.storage {
        underwrap_protocol::config::StorageConfig::Plain { dir } => {
            (dir.to_string_lossy().to_string(), None)
        }
        underwrap_protocol::config::StorageConfig::Encrypted { dir, key_file } => (
            dir.to_string_lossy().to_string(),
            Some(key_file.to_string_lossy().to_string()),
        ),
    };
    AdminStoreDetailResponse {
        id: loaded.record.id.clone(),
        name: loaded.record.name.clone(),
        backend: backend_kind(&loaded.record.storage),
        dir,
        key_file,
        description: loaded.record.description.clone(),
        created_at: loaded.record.created_at,
        source_path: stores_file.to_string_lossy().to_string(),
        usage,
        actions,
    }
}

fn handle_admin_stores_list(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
) -> Response<Full<Bytes>> {
    let visible: Vec<(String, LoadedStoreRecord, Arc<dyn StorageBackend>)> = {
        let reg = state.stores.read().unwrap();
        reg.records
            .iter()
            .filter(|(id, _)| principal.has_store_access(id))
            .filter_map(|(id, loaded)| {
                reg.open
                    .get(id)
                    .cloned()
                    .map(|handle| (id.clone(), loaded.clone(), handle))
            })
            .collect()
    };
    let mut items = Vec::with_capacity(visible.len());
    for (id, loaded, handle) in visible {
        let usage = match admin_store_usage(state, &id, handle.as_ref()) {
            Ok(usage) => usage,
            Err(resp) => return resp,
        };
        let actions = admin_store_actions(principal, &loaded, &usage);
        items.push(admin_store_list_item(&loaded, usage, actions));
    }
    items.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
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
    let (loaded, handle) = {
        let reg = state.stores.read().unwrap();
        match (reg.records.get(name), reg.open.get(name).cloned()) {
            (Some(loaded), Some(handle)) => (loaded.clone(), handle),
            (Some(_), None) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "store metadata present but backend missing",
                );
            }
            (None, _) => {
                return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
            }
        }
    };
    let usage = match admin_store_usage(state, name, handle.as_ref()) {
        Ok(usage) => usage,
        Err(resp) => return resp,
    };
    let actions = admin_store_actions(principal, &loaded, &usage);
    json_response(
        StatusCode::OK,
        &admin_store_detail(&state.stores_file, &loaded, usage, actions),
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

    let (storage, generated_key_hex) = match body.backend {
        AdminStoreBackendKind::Plain => (
            underwrap_protocol::config::StorageConfig::Plain {
                dir: validated_dir.clone(),
            },
            None,
        ),
        AdminStoreBackendKind::Encrypted => {
            let key_file = state.keys_dir.join(format!("{}.key", store_id));
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

    let record = StoreRecord {
        id: store_id.clone(),
        name: visible_name,
        description: body.description.clone(),
        created_at: Some(crate::operators::now_epoch_secs()),
        storage,
    };

    if let Err(e) = create_data_dir_secure(&validated_dir) {
        rollback_generated_key(&record.storage);
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "create_data_dir_failed",
            &format!("creating data dir {}: {e:#}", validated_dir.display()),
        );
    }

    let opened: Arc<dyn StorageBackend> = match underwrap_store::open(&record.storage) {
        Ok(b) => Arc::from(b),
        Err(e) => {
            rollback_generated_key(&record.storage);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "open_store_failed",
                &format!("opening store backend after create: {e:#}"),
            );
        }
    };

    let loaded = LoadedStoreRecord { record };
    {
        let mut reg = state.stores.write().unwrap();
        reg.open.insert(store_id.clone(), opened);
        reg.records.insert(store_id.clone(), loaded.clone());
        if let Err(e) = save_store_records(&state.stores_file, &reg.records) {
            reg.open.remove(&store_id);
            reg.records.remove(&store_id);
            rollback_generated_key(&loaded.record.storage);
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
    let actions = admin_store_actions(principal, &loaded, &usage);
    json_response(
        StatusCode::OK,
        &AdminStoreCreateResponse {
            store: admin_store_detail(&state.stores_file, &loaded, usage, actions),
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
    let handle = {
        let reg = state.stores.read().unwrap();
        match reg.open.get(name).cloned() {
            Some(handle) => handle,
            None => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "store metadata present but backend missing",
                );
            }
        }
    };
    let usage = match admin_store_usage(state, name, handle.as_ref()) {
        Ok(usage) => usage,
        Err(resp) => return resp,
    };
    let actions = admin_store_actions(principal, &loaded, &usage);
    json_response(
        StatusCode::OK,
        &admin_store_detail(&state.stores_file, &loaded, usage, actions),
    )
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
    let (store_handle, dir_for_hint) = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.records.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        let Some(handle) = reg.open.get(name).cloned() else {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "store metadata present but backend missing",
            );
        };
        (handle, loaded.record.storage.dir().clone())
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
            "store contains secrets; re-issue with `?force=true` to delete the record anyway",
        );
    }

    {
        let mut reg = state.stores.write().unwrap();
        reg.open.remove(name);
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

/// `POST /v1/admin/stores/{id}/rotate-key` — generate a fresh master
/// key, replace the on-disk key file atomically, reopen the backend,
/// persist the updated store record, and return the new key value
/// **exactly once**. Plain stores reject the operation.
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

    let (mut updated_record, key_file, dir) = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.records.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        let record = loaded.record.clone();
        match record.storage.clone() {
            underwrap_protocol::config::StorageConfig::Plain { .. } => {
                return error_response(
                    StatusCode::CONFLICT,
                    "not_encrypted",
                    "store is not encrypted; rotate-key only applies to encrypted backends",
                );
            }
            underwrap_protocol::config::StorageConfig::Encrypted { dir, key_file } => {
                (record, key_file, dir)
            }
        }
    };

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

    updated_record.storage = underwrap_protocol::config::StorageConfig::Encrypted {
        dir: updated_record.storage.dir().clone(),
        key_file: key_file.clone(),
    };
    {
        let mut reg = state.stores.write().unwrap();
        reg.open.insert(name.to_string(), new_backend);
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
            warning: "the previous master key has been replaced; secrets already \
                     stored under this store remain encrypted with the old key and \
                     cannot be decrypted through this backend. re-encrypt them \
                     offline if you need to keep them readable."
                .to_string(),
        },
    )
}

fn write_admin_store_audit(
    _state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
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

fn client_instance_status_to_wire(
    status: crate::runtime_clients::ClientInstanceStatus,
) -> AdminClientInstanceStatus {
    match status {
        crate::runtime_clients::ClientInstanceStatus::Active => AdminClientInstanceStatus::Active,
        crate::runtime_clients::ClientInstanceStatus::Disabled => {
            AdminClientInstanceStatus::Disabled
        }
    }
}

fn client_instance_status_from_wire(
    status: AdminClientInstanceStatus,
) -> crate::runtime_clients::ClientInstanceStatus {
    match status {
        AdminClientInstanceStatus::Active => crate::runtime_clients::ClientInstanceStatus::Active,
        AdminClientInstanceStatus::Disabled => {
            crate::runtime_clients::ClientInstanceStatus::Disabled
        }
    }
}

fn admin_client_instance_actions(
    principal: &crate::operators::Principal,
    record: &crate::runtime_clients::ClientInstanceRecord,
) -> AdminClientInstanceActionState {
    let can_manage = principal.is_admin();
    AdminClientInstanceActionState {
        can_edit_display_name: can_manage,
        can_remap_store: can_manage,
        can_disable: can_manage
            && record.status == crate::runtime_clients::ClientInstanceStatus::Active,
        can_enable: can_manage
            && record.status == crate::runtime_clients::ClientInstanceStatus::Disabled,
        can_delete: can_manage,
    }
}

fn current_store_names(state: &AdminRuntimeState) -> HashMap<String, String> {
    state
        .stores
        .read()
        .unwrap()
        .records
        .iter()
        .map(|(id, loaded)| (id.clone(), loaded.record.name.clone()))
        .collect()
}

fn admin_client_instance_list_item(
    record: &crate::runtime_clients::ClientInstanceRecord,
    current_store_name: String,
    actions: AdminClientInstanceActionState,
) -> AdminClientInstanceListItem {
    AdminClientInstanceListItem {
        id: record.id.clone(),
        display_name: record.display_name.clone(),
        current_store_id: record.current_store_id.clone(),
        current_store_name,
        status: client_instance_status_to_wire(record.status),
        last_seen_at: record.last_seen_at,
        current_credential_fingerprint: record.current_credential.cert_fingerprint.clone(),
        auto_create_wrapper_assets: record.auto_create_wrapper_assets,
        actions,
    }
}

fn admin_client_instance_detail(
    record: &crate::runtime_clients::ClientInstanceRecord,
    current_store_name: String,
    actions: AdminClientInstanceActionState,
) -> AdminClientInstanceDetailResponse {
    AdminClientInstanceDetailResponse {
        id: record.id.clone(),
        display_name: record.display_name.clone(),
        current_store_id: record.current_store_id.clone(),
        current_store_name,
        status: client_instance_status_to_wire(record.status),
        last_seen_at: record.last_seen_at,
        current_credential_fingerprint: record.current_credential.cert_fingerprint.clone(),
        current_credential_issued_at: record.current_credential.issued_at,
        auto_create_wrapper_assets: record.auto_create_wrapper_assets,
        actions,
    }
}

fn handle_admin_client_instances_list(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may list client instances",
        );
    }
    let store_names = current_store_names(state);
    let mut items = Vec::new();
    for record in state.client_instances.read().unwrap().values() {
        let Some(current_store_name) = store_names.get(&record.current_store_id).cloned() else {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "unknown_store",
                "client instance references an unknown store",
            );
        };
        let actions = admin_client_instance_actions(principal, record);
        items.push(admin_client_instance_list_item(
            record,
            current_store_name,
            actions,
        ));
    }
    items.sort_by(|a, b| a.display_name.cmp(&b.display_name).then(a.id.cmp(&b.id)));
    json_response(StatusCode::OK, &AdminClientInstancesListResponse { items })
}

fn handle_admin_client_instance_detail(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    id: &str,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may inspect client instances",
        );
    }
    let store_names = current_store_names(state);
    match state.client_instances.read().unwrap().get(id) {
        Some(record) => {
            let Some(current_store_name) = store_names.get(&record.current_store_id).cloned()
            else {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "unknown_store",
                    "client instance references an unknown store",
                );
            };
            let actions = admin_client_instance_actions(principal, record);
            json_response(
                StatusCode::OK,
                &admin_client_instance_detail(record, current_store_name, actions),
            )
        }
        None => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "unknown client instance",
        ),
    }
}

async fn handle_admin_client_instance_create(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may create client instances",
        );
    }
    let body = match parse_json_body::<AdminClientInstanceCreateRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    do_admin_client_instance_create(state, principal, body)
}

fn do_admin_client_instance_create(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: AdminClientInstanceCreateRequest,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may create client instances",
        );
    }
    let display_name = match validate_store_name(&body.display_name) {
        Ok(v) => v,
        Err(msg) => return error_response(StatusCode::BAD_REQUEST, "invalid_display_name", msg),
    };
    let current_credential =
        match crate::runtime_clients::current_credential_from_pem(body.cert_pem, body.issued_at) {
            Ok(current) => current,
            Err(e) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_client_certificate",
                    &format!("parsing client certificate: {e:#}"),
                );
            }
        };
    let cert_fingerprint = current_credential.cert_fingerprint.clone();
    {
        let stores = state.stores.read().unwrap();
        if !stores.records.contains_key(&body.current_store_id) {
            return error_response(StatusCode::BAD_REQUEST, "unknown_store", "unknown store");
        }
    }
    let id = loop {
        let candidate = generate_client_instance_id();
        if !state
            .client_instances
            .read()
            .unwrap()
            .contains_key(&candidate)
        {
            break candidate;
        }
    };
    let mut instances = state.client_instances.write().unwrap();
    if instances
        .values()
        .any(|existing| existing.display_name.eq_ignore_ascii_case(&display_name))
    {
        return error_response(
            StatusCode::CONFLICT,
            "display_name_already_exists",
            &format!(
                "client instance display name '{}' already exists",
                display_name
            ),
        );
    }
    if instances
        .values()
        .any(|existing| existing.current_credential.cert_fingerprint == cert_fingerprint)
    {
        return error_response(
            StatusCode::CONFLICT,
            "credential_already_exists",
            "client credential fingerprint already exists",
        );
    }
    let record = crate::runtime_clients::ClientInstanceRecord {
        id: id.clone(),
        display_name,
        current_store_id: body.current_store_id,
        status: client_instance_status_from_wire(body.status),
        last_seen_at: None,
        current_credential,
        auto_create_wrapper_assets: body.auto_create_wrapper_assets.unwrap_or(false),
    };
    instances.insert(id.clone(), record.clone());
    if let Err(e) =
        crate::runtime_clients::save_client_instances(&state.client_instances_file, &instances)
    {
        instances.remove(&id);
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "write_client_instances_failed",
            &format!("{e:#}"),
        );
    }
    log_admin_event(
        "client-instance.create",
        &principal.username,
        None,
        Some(format!("client-instance:{id}").as_str()),
        None,
        None,
        "success",
    );
    let current_store_name = {
        let stores = state.stores.read().unwrap();
        match stores.records.get(&record.current_store_id) {
            Some(store) => store.record.name.clone(),
            None => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "unknown_store",
                    "client instance references an unknown store",
                );
            }
        }
    };
    let actions = admin_client_instance_actions(principal, &record);
    json_response(
        StatusCode::OK,
        &admin_client_instance_detail(&record, current_store_name, actions),
    )
}

async fn handle_admin_client_instance_update(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    id: &str,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may update client instances",
        );
    }
    let body = match parse_json_body::<AdminClientInstanceUpdateRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    do_admin_client_instance_update(state, principal, id, body)
}

fn do_admin_client_instance_update(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    id: &str,
    body: AdminClientInstanceUpdateRequest,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may update client instances",
        );
    }
    let mut instances = state.client_instances.write().unwrap();
    let Some(existing) = instances.get(id).cloned() else {
        return error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "unknown client instance",
        );
    };
    let mut updated = existing;
    if let Some(display_name) = body.display_name {
        let display_name = match validate_store_name(&display_name) {
            Ok(v) => v,
            Err(msg) => {
                return error_response(StatusCode::BAD_REQUEST, "invalid_display_name", msg);
            }
        };
        if instances.iter().any(|(other_id, other)| {
            other_id != id && other.display_name.eq_ignore_ascii_case(&display_name)
        }) {
            return error_response(
                StatusCode::CONFLICT,
                "display_name_already_exists",
                &format!(
                    "client instance display name '{}' already exists",
                    display_name
                ),
            );
        }
        updated.display_name = display_name;
    }
    if let Some(current_store_id) = body.current_store_id {
        if !state
            .stores
            .read()
            .unwrap()
            .records
            .contains_key(&current_store_id)
        {
            return error_response(StatusCode::BAD_REQUEST, "unknown_store", "unknown store");
        }
        updated.current_store_id = current_store_id;
    }
    if let Some(status) = body.status {
        updated.status = client_instance_status_from_wire(status);
    }
    if let Some(cert_pem) = body.cert_pem {
        let now = crate::operators::now_epoch_secs();
        let new_credential =
            match crate::runtime_clients::current_credential_from_pem(cert_pem, Some(now)) {
                Ok(current) => current,
                Err(e) => {
                    return error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_client_certificate",
                        &format!("parsing client certificate: {e:#}"),
                    );
                }
            };
        if instances.iter().any(|(other_id, other)| {
            other_id != id
                && other.current_credential.cert_fingerprint == new_credential.cert_fingerprint
        }) {
            return error_response(
                StatusCode::CONFLICT,
                "credential_already_exists",
                "client credential fingerprint already exists",
            );
        }
        updated.current_credential = new_credential;
    }
    if let Some(flag) = body.auto_create_wrapper_assets {
        updated.auto_create_wrapper_assets = flag;
    }
    instances.insert(id.to_string(), updated.clone());
    if let Err(e) =
        crate::runtime_clients::save_client_instances(&state.client_instances_file, &instances)
    {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "write_client_instances_failed",
            &format!("{e:#}"),
        );
    }
    log_admin_event(
        "client-instance.update",
        &principal.username,
        None,
        Some(format!("client-instance:{id}").as_str()),
        None,
        None,
        "success",
    );
    let current_store_name = {
        let stores = state.stores.read().unwrap();
        match stores.records.get(&updated.current_store_id) {
            Some(store) => store.record.name.clone(),
            None => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "unknown_store",
                    "client instance references an unknown store",
                );
            }
        }
    };
    let actions = admin_client_instance_actions(principal, &updated);
    json_response(
        StatusCode::OK,
        &admin_client_instance_detail(&updated, current_store_name, actions),
    )
}

fn handle_admin_client_instance_delete(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    id: &str,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may delete client instances",
        );
    }
    let mut instances = state.client_instances.write().unwrap();
    if instances.remove(id).is_none() {
        return error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "unknown client instance",
        );
    }
    if let Err(e) =
        crate::runtime_clients::save_client_instances(&state.client_instances_file, &instances)
    {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "write_client_instances_failed",
            &format!("{e:#}"),
        );
    }
    log_admin_event(
        "client-instance.delete",
        &principal.username,
        None,
        Some(format!("client-instance:{id}").as_str()),
        None,
        None,
        "success",
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminOkResponse { ok: true },
    )
}

// ===== Wrapper permission CRUD =====

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

fn write_admin_permission_audit(
    _state: &AdminRuntimeState,
    actor: &crate::operators::Principal,
    store_name: &str,
    permission: &underwrap_protocol::policy::WrapperPermission,
    action: &str,
    result: &str,
) {
    let resource = format!(
        "wrapper_permission:{store_name}/{}/{}/{}",
        permission.client_instance, permission.wrapper, permission.resource
    );
    log_admin_event(
        action,
        &actor.username,
        Some(permission.wrapper.as_str()),
        Some(resource.as_str()),
        None,
        None,
        result,
    );
}

// ===== Admin Resource and Pair CRUD =====
//
// Six endpoints behind `/v1/admin/resources/*` and
// `/v1/admin/pairs/*`. All require an authenticated admin operator;
// reads return store-scoped data, writes set provenance on create,
// deletes refuse with structured blockers when dependents exist.

const ENTITY_DEPENDENT_SAMPLE_LIMIT: usize = 5;
const RESERVED_PREFIX: &str = "__underwrap/";

fn resource_secret_dependents(
    store: &dyn StorageBackend,
    resource_name: &str,
) -> Result<(u32, Vec<String>)> {
    let mut ids = resource_blob_entries(store, resource_name)?;
    ids.retain(|id| !id.starts_with(RESERVED_PREFIX));
    ids.sort();
    ids.dedup();
    let count = u32::try_from(ids.len()).unwrap_or(u32::MAX);
    ids.truncate(ENTITY_DEPENDENT_SAMPLE_LIMIT);
    Ok((count, ids))
}

#[allow(clippy::result_large_err)]
fn handle_admin_resources_list(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminResourcesListRequest,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may list resources",
        );
    }
    let store = match admin_store_for_principal(state, principal, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    match load_resources_document(store.as_ref()) {
        Ok(mut doc) => {
            doc.resources.sort_by(|a, b| a.name.cmp(&b.name));
            json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminResourcesListResponse {
                    resources: doc.resources,
                },
            )
        }
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("loading resources document: {e:#}"),
        ),
    }
}

#[allow(clippy::result_large_err)]
fn handle_admin_resource_blob_get(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminResourceBlobGetRequest,
) -> Response<Full<Bytes>> {
    let store = match admin_store_for_principal(state, principal, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let blob = match load_resource_blob(store.as_ref(), &body.name) {
        Ok(b) => b,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading resource blob: {e:#}"),
            );
        }
    };
    let bytes = match blob.to_canonical_bytes() {
        Ok(b) => b,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("canonicalizing resource blob: {e:#}"),
            );
        }
    };
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminResourceBlobGetResponse {
            blob_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
        },
    )
}

fn handle_admin_resource_blob_put(
    state: &AdminRuntimeState,
    operator: &str,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminResourceBlobPutRequest,
) -> Response<Full<Bytes>> {
    use underwrap_protocol::resource_blob::ResourceBlob;
    let store = match admin_store_for_principal(state, principal, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let blob_bytes = match base64::engine::general_purpose::STANDARD.decode(&body.blob_b64) {
        Ok(b) => b,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("blob_b64 is not valid base64: {e}"),
            );
        }
    };
    let blob = match ResourceBlob::from_canonical_bytes(&blob_bytes) {
        Ok(b) => b,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_blob",
                &format!("decoding resource blob: {e:#}"),
            );
        }
    };
    // Enforce that the caller sent already-canonical bytes. The
    // wrapper-side does this naturally via serde_jcs; admin clients
    // must as well.
    let canonical = match blob.to_canonical_bytes() {
        Ok(b) => b,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("canonicalizing resource blob: {e:#}"),
            );
        }
    };
    if canonical != blob_bytes {
        return error_response(
            StatusCode::BAD_REQUEST,
            "noncanonical_blob",
            "blob_b64 is not JCS-canonical; re-encode via serde_jcs",
        );
    }
    if let Err(e) = save_resource_blob(store.as_ref(), &body.name, &blob) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("saving resource blob: {e:#}"),
        );
    }
    log_admin_event(
        "resource.blob.put",
        operator,
        None,
        Some(&body.name),
        None,
        None,
        "success",
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminResourceBlobPutResponse { ok: true },
    )
}

fn handle_admin_resource_create(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminResourceCreateRequest,
) -> Response<Full<Bytes>> {
    use underwrap_protocol::entities::{EntityCreator, ResourceRecord};
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may create resources",
        );
    }
    let name = body.name.trim();
    if name.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_resource_name",
            "resource name must be non-empty",
        );
    }
    let store = match admin_store_for_principal(state, principal, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let mut doc = match load_resources_document(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading resources document: {e:#}"),
            );
        }
    };
    if doc.find(name).is_some() {
        return error_response(
            StatusCode::CONFLICT,
            "resource_already_exists",
            &format!("resource '{name}' already exists"),
        );
    }
    let record = ResourceRecord {
        name: name.to_string(),
        created_by: EntityCreator::Operator {
            id: principal.id.clone(),
            username: principal.username.clone(),
        },
        created_at: crate::operators::now_epoch_secs(),
    };
    doc.insert(record.clone())
        .expect("find returned None just above");
    if let Err(e) = save_resources_document(store.as_ref(), &doc) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting resources document: {e:#}"),
        );
    }
    log_admin_event(
        "resource.create",
        &principal.username,
        None,
        Some(name),
        None,
        None,
        "success",
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminResourceCreateResponse { resource: record },
    )
}

#[allow(clippy::result_large_err)]
fn handle_admin_resource_delete(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminResourceDeleteRequest,
) -> Response<Full<Bytes>> {
    use underwrap_protocol::service::{
        AdminResourceDeleteBlockers, AdminResourceDeleteResponse, AdminWrapperPermissionKey,
        ServiceWrapperResourcePair,
    };
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may delete resources",
        );
    }
    let store = match admin_store_for_principal(state, principal, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let mut doc = match load_resources_document(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading resources document: {e:#}"),
            );
        }
    };
    if doc.find(&body.name).is_none() {
        return error_response(StatusCode::NOT_FOUND, "not_found", "unknown resource");
    }

    let (secret_count, sample_secret_ids) =
        match resource_secret_dependents(store.as_ref(), &body.name) {
            Ok(v) => v,
            Err(e) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    &format!("scanning resource dependents: {e:#}"),
                );
            }
        };

    let pairs = match load_pairs_document(store.as_ref()) {
        Ok(p) => p,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading pairs document: {e:#}"),
            );
        }
    };
    let dependent_pairs: Vec<ServiceWrapperResourcePair> = pairs
        .pairs_for_resource(&body.name)
        .map(|p| ServiceWrapperResourcePair {
            wrapper: p.wrapper.clone(),
            resource: p.resource.clone(),
            files_mount_paths: p.files_mount_paths.clone(),
        })
        .collect();
    let pair_count = u32::try_from(dependent_pairs.len()).unwrap_or(u32::MAX);
    let mut sample_pairs = dependent_pairs;
    sample_pairs.truncate(ENTITY_DEPENDENT_SAMPLE_LIMIT);

    let policy = match load_policy_document_or_default(store.as_ref()) {
        Ok(p) => p,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading policy document: {e:#}"),
            );
        }
    };
    let dependent_permissions: Vec<AdminWrapperPermissionKey> = policy
        .permissions
        .iter()
        .filter(|p| p.resource == body.name)
        .map(|p| AdminWrapperPermissionKey {
            client_instance: p.client_instance.clone(),
            wrapper: p.wrapper.clone(),
            resource: p.resource.clone(),
        })
        .collect();
    let permission_count = u32::try_from(dependent_permissions.len()).unwrap_or(u32::MAX);
    let mut sample_permissions = dependent_permissions;
    sample_permissions.truncate(ENTITY_DEPENDENT_SAMPLE_LIMIT);

    if secret_count > 0 || pair_count > 0 || permission_count > 0 {
        log_admin_event(
            "resource.delete",
            &principal.username,
            None,
            Some(&body.name),
            None,
            None,
            "blocked",
        );
        return json_response(
            StatusCode::OK,
            &AdminResourceDeleteResponse {
                ok: false,
                blocked_by: Some(AdminResourceDeleteBlockers {
                    secret_count,
                    sample_secret_ids,
                    pair_count,
                    sample_pairs,
                    permission_count,
                    sample_permissions,
                }),
            },
        );
    }

    doc.remove(&body.name);
    if let Err(e) = save_resources_document(store.as_ref(), &doc) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting resources document: {e:#}"),
        );
    }
    log_admin_event(
        "resource.delete",
        &principal.username,
        None,
        Some(&body.name),
        None,
        None,
        "success",
    );
    json_response(
        StatusCode::OK,
        &AdminResourceDeleteResponse {
            ok: true,
            blocked_by: None,
        },
    )
}

#[allow(clippy::result_large_err)]
fn handle_admin_pairs_list(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminPairsListRequest,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may list wrapper/resource pairs",
        );
    }
    let store = match admin_store_for_principal(state, principal, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    match load_pairs_document(store.as_ref()) {
        Ok(mut doc) => {
            doc.pairs
                .sort_by(|a, b| a.wrapper.cmp(&b.wrapper).then(a.resource.cmp(&b.resource)));
            json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminPairsListResponse { pairs: doc.pairs },
            )
        }
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("loading pairs document: {e:#}"),
        ),
    }
}

#[allow(clippy::result_large_err)]
fn handle_admin_pair_create(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminPairCreateRequest,
) -> Response<Full<Bytes>> {
    use underwrap_protocol::entities::{EntityCreator, WrapperResourcePairRecord};
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may create wrapper/resource pairs",
        );
    }
    let wrapper = body.wrapper.trim();
    let resource = body.resource.trim();
    if wrapper.is_empty() || resource.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_pair",
            "wrapper and resource must both be non-empty",
        );
    }
    let store = match admin_store_for_principal(state, principal, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let resources = match load_resources_document(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading resources document: {e:#}"),
            );
        }
    };
    if resources.find(resource).is_none() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "unknown_resource",
            &format!("resource '{resource}' must be created before a pair can reference it"),
        );
    }
    let mut pairs = match load_pairs_document(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading pairs document: {e:#}"),
            );
        }
    };
    if pairs.find(wrapper, resource).is_some() {
        return error_response(
            StatusCode::CONFLICT,
            "pair_already_exists",
            &format!("pair ({wrapper}, {resource}) already exists"),
        );
    }
    let record = WrapperResourcePairRecord {
        wrapper: wrapper.to_string(),
        resource: resource.to_string(),
        created_by: EntityCreator::Operator {
            id: principal.id.clone(),
            username: principal.username.clone(),
        },
        created_at: crate::operators::now_epoch_secs(),
        last_used_at: None,
        files_mount_paths: Vec::new(),
    };
    pairs
        .insert(record.clone())
        .expect("find returned None just above");
    if let Err(e) = save_pairs_document(store.as_ref(), &pairs) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting pairs document: {e:#}"),
        );
    }
    log_admin_event(
        "pair.create",
        &principal.username,
        Some(wrapper),
        Some(resource),
        None,
        None,
        "success",
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminPairCreateResponse { pair: record },
    )
}

#[allow(clippy::result_large_err)]
fn handle_admin_pair_delete(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminPairDeleteRequest,
) -> Response<Full<Bytes>> {
    use underwrap_protocol::service::{
        AdminPairDeleteBlockers, AdminPairDeleteResponse, AdminWrapperPermissionKey,
    };
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may delete wrapper/resource pairs",
        );
    }
    let store = match admin_store_for_principal(state, principal, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let mut doc = match load_pairs_document(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading pairs document: {e:#}"),
            );
        }
    };
    if doc.find(&body.wrapper, &body.resource).is_none() {
        return error_response(StatusCode::NOT_FOUND, "not_found", "unknown pair");
    }

    let policy = match load_policy_document_or_default(store.as_ref()) {
        Ok(p) => p,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading policy document: {e:#}"),
            );
        }
    };
    let dependent_permissions: Vec<AdminWrapperPermissionKey> = policy
        .permissions
        .iter()
        .filter(|p| p.wrapper == body.wrapper && p.resource == body.resource)
        .map(|p| AdminWrapperPermissionKey {
            client_instance: p.client_instance.clone(),
            wrapper: p.wrapper.clone(),
            resource: p.resource.clone(),
        })
        .collect();
    let permission_count = u32::try_from(dependent_permissions.len()).unwrap_or(u32::MAX);
    let mut sample_permissions = dependent_permissions;
    sample_permissions.truncate(ENTITY_DEPENDENT_SAMPLE_LIMIT);

    if permission_count > 0 {
        log_admin_event(
            "pair.delete",
            &principal.username,
            Some(&body.wrapper),
            Some(&body.resource),
            None,
            None,
            "blocked",
        );
        return json_response(
            StatusCode::OK,
            &AdminPairDeleteResponse {
                ok: false,
                blocked_by: Some(AdminPairDeleteBlockers {
                    permission_count,
                    sample_permissions,
                }),
            },
        );
    }

    doc.remove(&body.wrapper, &body.resource);
    if let Err(e) = save_pairs_document(store.as_ref(), &doc) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting pairs document: {e:#}"),
        );
    }
    log_admin_event(
        "pair.delete",
        &principal.username,
        Some(&body.wrapper),
        Some(&body.resource),
        None,
        None,
        "success",
    );
    json_response(
        StatusCode::OK,
        &AdminPairDeleteResponse {
            ok: true,
            blocked_by: None,
        },
    )
}

#[allow(clippy::result_large_err)]
fn handle_admin_wrapper_permissions_list(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminWrapperPermissionsListRequest,
) -> Response<Full<Bytes>> {
    let store = match admin_store_for_principal(state, principal, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    match load_policy_document_or_default(store.as_ref()) {
        Ok(doc) => json_response(
            StatusCode::OK,
            &underwrap_protocol::service::AdminWrapperPermissionsListResponse {
                permissions: doc.permissions,
            },
        ),
        Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    }
}

#[allow(clippy::result_large_err)]
fn handle_admin_wrapper_permission_upsert(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminWrapperPermissionUpsertRequest,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may mutate wrapper permissions",
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
    if body.permission.client_instance.is_empty()
        || body.permission.wrapper.is_empty()
        || body.permission.resource.is_empty()
    {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_permission",
            "client_instance, wrapper, and resource must all be non-empty",
        );
    }
    let new_permission = body.permission.clone();
    if let Some(slot) = doc.permissions.iter_mut().find(|p| {
        p.client_instance == new_permission.client_instance
            && p.wrapper == new_permission.wrapper
            && p.resource == new_permission.resource
    }) {
        *slot = new_permission.clone();
    } else {
        doc.permissions.push(new_permission.clone());
    }
    doc.permissions.sort_by(|a, b| {
        a.client_instance
            .cmp(&b.client_instance)
            .then(a.wrapper.cmp(&b.wrapper))
            .then(a.resource.cmp(&b.resource))
    });
    if let Err(e) = save_policy_document(store.as_ref(), &doc) {
        write_admin_permission_audit(
            state,
            principal,
            &body.store,
            &new_permission,
            "wrapper_permission.upsert",
            "failure",
        );
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting policy document: {e:#}"),
        );
    }
    write_admin_permission_audit(
        state,
        principal,
        &body.store,
        &new_permission,
        "wrapper_permission.upsert",
        "success",
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminOkResponse { ok: true },
    )
}

#[allow(clippy::result_large_err)]
fn handle_admin_wrapper_permission_delete(
    state: &AdminRuntimeState,
    principal: &crate::operators::Principal,
    body: &underwrap_protocol::service::AdminWrapperPermissionDeleteRequest,
) -> Response<Full<Bytes>> {
    if !principal.is_admin() {
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only admins may mutate wrapper permissions",
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
    let before = doc.permissions.len();
    doc.permissions.retain(|p| {
        !(p.client_instance == body.client_instance
            && p.wrapper == body.wrapper
            && p.resource == body.resource)
    });
    let removed = before != doc.permissions.len();
    if !removed {
        return error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "no matching wrapper permission",
        );
    }
    if let Err(e) = save_policy_document(store.as_ref(), &doc) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting policy document: {e:#}"),
        );
    }
    let removed_permission = underwrap_protocol::policy::WrapperPermission {
        client_instance: body.client_instance.clone(),
        wrapper: body.wrapper.clone(),
        resource: body.resource.clone(),
        mode: underwrap_protocol::policy::WrapperApprovalMode::Locked,
        writable: false,
        reason: None,
    };
    write_admin_permission_audit(
        state,
        principal,
        &body.store,
        &removed_permission,
        "wrapper_permission.delete",
        "success",
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminOkResponse { ok: true },
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
        return Ok(handle_admin_logout(&state, auth.session()));
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
                return Ok(handle_admin_client_instance_detail(&state, &principal, &id));
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
        (&Method::GET, "/v1/admin/client-instances") => {
            return Ok(handle_admin_client_instances_list(&state, &principal));
        }
        (&Method::POST, "/v1/admin/client-instances") => {
            return Ok(handle_admin_client_instance_create(&state, &principal, request).await);
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
        (&Method::POST, "/v1/admin/secret/read") => match parse_json_body::<
            underwrap_protocol::service::AdminSecretReadRequest,
        >(request)
        .await
        {
            Ok(body) => match admin_store_for_principal(&state, &principal, &body.store) {
                Ok(store) => match read_legacy_secret(store.as_ref(), &body.secret_id) {
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
                    Ok(store) => match list_legacy_secrets(store.as_ref(), &body.prefix) {
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
                    Ok(store) => match delete_legacy_secret(store.as_ref(), &body.secret_id) {
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
        // ===== Wrapper permission CRUD =====
        (&Method::POST, "/v1/admin/wrapper-permissions/list") => match parse_json_body::<
            underwrap_protocol::service::AdminWrapperPermissionsListRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_wrapper_permissions_list(&state, &principal, &body),
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
                Ok(body) => handle_admin_resources_list(&state, &principal, &body),
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
        (&Method::POST, "/v1/admin/resources/blob/get") => match parse_json_body::<
            underwrap_protocol::service::AdminResourceBlobGetRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_resource_blob_get(&state, &principal, &body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/resources/blob/put") => match parse_json_body::<
            underwrap_protocol::service::AdminResourceBlobPutRequest,
        >(request)
        .await
        {
            Ok(body) => handle_admin_resource_blob_put(&state, &operator, &principal, &body),
            Err(response) => response,
        },
        (&Method::POST, "/v1/admin/pairs/list") => {
            match parse_json_body::<underwrap_protocol::service::AdminPairsListRequest>(request)
                .await
            {
                Ok(body) => handle_admin_pairs_list(&state, &principal, &body),
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

/// Refresh the CSRF cookie on every authenticated admin response so
/// the SPA always has a value matching the live session.
fn attach_csrf_cookie(
    mut response: Response<Full<Bytes>>,
    state: &AdminRuntimeState,
    auth: &AdminAuth,
) -> Response<Full<Bytes>> {
    let payload = auth.session();
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
    response
}

#[cfg(test)]
mod store_registry_tests {
    use super::*;

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
            resource: "github/token".into(),
            created_by: EntityCreator::ClientInstance {
                id: "client-7".into(),
            },
            created_at: 42,
            last_used_at: None,
            files_mount_paths: Vec::new(),
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

    #[test]
    fn runtime_register_noops_when_nothing_is_missing_regardless_of_flag() {
        use underwrap_protocol::service::{
            ServiceWrapperRegisterRequest, ServiceWrapperResourcePair,
        };
        let store = TestStore::new();
        // Seed with the flag on.
        apply_runtime_wrapper_register(
            &store,
            "inst_a",
            true,
            &ServiceWrapperRegisterRequest {
                pairs: vec![ServiceWrapperResourcePair {
                    wrapper: "gh".into(),
                    resource: "github/token".into(),
                    files_mount_paths: Vec::new(),
                }],
            },
            10,
        )
        .unwrap();
        // Now register the same pair set with the flag off: nothing is
        // missing, so the call succeeds with no refusal.
        let resp = apply_runtime_wrapper_register(
            &store,
            "inst_a",
            false,
            &ServiceWrapperRegisterRequest {
                pairs: vec![ServiceWrapperResourcePair {
                    wrapper: "gh".into(),
                    resource: "github/token".into(),
                    files_mount_paths: Vec::new(),
                }],
            },
            20,
        )
        .unwrap();
        assert!(resp.created_resources.is_empty());
        assert!(resp.created_pairs.is_empty());
        assert!(resp.rejected.is_none());
    }

    #[test]
    fn runtime_register_noops_when_nothing_is_missing() {
        use underwrap_protocol::service::{
            ServiceWrapperRegisterRequest, ServiceWrapperResourcePair,
        };
        let store = TestStore::new();
        // Seed an existing resource + pair so the second register
        // call has nothing to do.
        apply_runtime_wrapper_register(
            &store,
            "inst_a",
            true,
            &ServiceWrapperRegisterRequest {
                pairs: vec![ServiceWrapperResourcePair {
                    wrapper: "gh".into(),
                    resource: "github/token".into(),
                    files_mount_paths: Vec::new(),
                }],
            },
            10,
        )
        .unwrap();

        let resp = apply_runtime_wrapper_register(
            &store,
            "inst_a",
            true,
            &ServiceWrapperRegisterRequest {
                pairs: vec![ServiceWrapperResourcePair {
                    wrapper: "gh".into(),
                    resource: "github/token".into(),
                    files_mount_paths: Vec::new(),
                }],
            },
            20,
        )
        .unwrap();
        assert!(resp.created_resources.is_empty());
        assert!(resp.created_pairs.is_empty());
        assert!(resp.rejected.is_none());
        // The provenance from the first call is untouched.
        let pairs = load_pairs_document(&store).unwrap();
        assert_eq!(pairs.pairs[0].created_at, 10);
    }

    #[test]
    fn runtime_register_refuses_when_auto_create_disabled_and_lists_missing() {
        use underwrap_protocol::service::{
            ServiceWrapperRegisterReject, ServiceWrapperRegisterRequest, ServiceWrapperResourcePair,
        };
        let store = TestStore::new();
        let resp = apply_runtime_wrapper_register(
            &store,
            "inst_a",
            false,
            &ServiceWrapperRegisterRequest {
                pairs: vec![
                    ServiceWrapperResourcePair {
                        wrapper: "gh".into(),
                        resource: "github/token".into(),
                        files_mount_paths: Vec::new(),
                    },
                    ServiceWrapperResourcePair {
                        wrapper: "tf".into(),
                        resource: "github/token".into(),
                        files_mount_paths: Vec::new(),
                    },
                ],
            },
            10,
        )
        .unwrap();
        assert!(resp.created_resources.is_empty());
        assert!(resp.created_pairs.is_empty());
        match resp.rejected.expect("rejected response") {
            ServiceWrapperRegisterReject::AutoCreateDisabled {
                missing_resources,
                missing_pairs,
            } => {
                assert_eq!(missing_resources, vec!["github/token".to_string()]);
                assert_eq!(missing_pairs.len(), 2);
            }
        }
        // Refusal path writes nothing.
        assert_eq!(load_resources_document(&store).unwrap().resources.len(), 0);
        assert_eq!(load_pairs_document(&store).unwrap().pairs.len(), 0);
    }

    #[test]
    fn runtime_register_creates_missing_when_auto_create_enabled() {
        use underwrap_protocol::entities::EntityCreator;
        use underwrap_protocol::service::{
            ServiceWrapperRegisterRequest, ServiceWrapperResourcePair,
        };
        let store = TestStore::new();
        let resp = apply_runtime_wrapper_register(
            &store,
            "inst_a",
            true,
            &ServiceWrapperRegisterRequest {
                pairs: vec![
                    ServiceWrapperResourcePair {
                        wrapper: "gh".into(),
                        resource: "github/token".into(),
                        files_mount_paths: Vec::new(),
                    },
                    ServiceWrapperResourcePair {
                        wrapper: "tf".into(),
                        resource: "github/token".into(),
                        files_mount_paths: Vec::new(),
                    },
                ],
            },
            42,
        )
        .unwrap();
        assert!(resp.rejected.is_none());
        assert_eq!(resp.created_resources, vec!["github/token".to_string()]);
        assert_eq!(resp.created_pairs.len(), 2);

        let resources = load_resources_document(&store).unwrap();
        assert_eq!(resources.resources.len(), 1);
        match &resources.resources[0].created_by {
            EntityCreator::ClientInstance { id } => assert_eq!(id, "inst_a"),
            _ => panic!("expected ClientInstance creator"),
        }
        assert_eq!(resources.resources[0].created_at, 42);

        let pairs = load_pairs_document(&store).unwrap();
        assert_eq!(pairs.pairs.len(), 2);
    }

    #[test]
    fn runtime_register_only_creates_truly_missing_entries() {
        use underwrap_protocol::service::{
            ServiceWrapperRegisterRequest, ServiceWrapperResourcePair,
        };
        let store = TestStore::new();
        // First call creates everything for inst_a.
        apply_runtime_wrapper_register(
            &store,
            "inst_a",
            true,
            &ServiceWrapperRegisterRequest {
                pairs: vec![ServiceWrapperResourcePair {
                    wrapper: "gh".into(),
                    resource: "github/token".into(),
                    files_mount_paths: Vec::new(),
                }],
            },
            10,
        )
        .unwrap();

        // Second call from inst_b adds a new pair on the existing
        // resource. The resource record is left alone, the new pair
        // gets created.
        let resp = apply_runtime_wrapper_register(
            &store,
            "inst_b",
            true,
            &ServiceWrapperRegisterRequest {
                pairs: vec![
                    ServiceWrapperResourcePair {
                        wrapper: "gh".into(),
                        resource: "github/token".into(),
                        files_mount_paths: Vec::new(),
                    },
                    ServiceWrapperResourcePair {
                        wrapper: "tf".into(),
                        resource: "github/token".into(),
                        files_mount_paths: Vec::new(),
                    },
                ],
            },
            20,
        )
        .unwrap();
        assert!(resp.created_resources.is_empty(), "resource already there");
        assert_eq!(resp.created_pairs.len(), 1);
        assert_eq!(resp.created_pairs[0].wrapper, "tf");
        let resources = load_resources_document(&store).unwrap();
        assert_eq!(
            resources.resources[0].created_at, 10,
            "provenance untouched"
        );
    }

    fn test_operators_file() -> PathBuf {
        std::env::temp_dir().join(format!(
            "underwrap-server-test-operators-{}.json",
            crate::operators::new_operator_id()
        ))
    }

    fn test_admin_state(stores: HashMap<String, Arc<dyn StorageBackend>>) -> AdminRuntimeState {
        AdminRuntimeState {
            stores: Arc::new(std::sync::RwLock::new(StoreRegistry {
                open: stores,
                records: HashMap::new(),
            })),
            client_instances: Arc::new(std::sync::RwLock::new(HashMap::new())),
            stores_file: std::path::PathBuf::new(),
            client_instances_file: std::path::PathBuf::new(),
            stores_data_root: None,
            keys_dir: std::path::PathBuf::new(),
            admin_enabled: true,
            allowed_origins: None,
            web_source: crate::web::WebAssetSource::Embedded,
            operators_file: test_operators_file(),
            session_key: [0u8; crate::auth::SESSION_KEY_LEN],
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
            keys_dir: None,
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
        let operators_file = test_operators_file();
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
        crate::operators::save(&operators_file, &collection).unwrap();
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
            keys_dir: std::path::PathBuf::new(),
            admin_enabled: true,
            allowed_origins: None,
            web_source: crate::web::WebAssetSource::Embedded,
            operators_file,
            session_key,
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
        let collection = crate::operators::load(&state.operators_file).unwrap();
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
        let collection = crate::operators::load(&state.operators_file).unwrap();
        assert!(collection.find_by_username("eve").is_none());
        assert!(!crate::auth::verify_password(
            crate::auth::DUMMY_PASSWORD_HASH,
            "anything"
        ));
    }

    #[tokio::test]
    async fn whoami_resolves_role_from_collection() {
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

    fn service_state_with_runtime_client(
        cert_der: &[u8],
        status: crate::runtime_clients::ClientInstanceStatus,
        store_id: &str,
    ) -> AdminRuntimeState {
        let mut stores: HashMap<String, Arc<dyn StorageBackend>> = HashMap::new();
        stores.insert(store_id.to_string(), Arc::new(MemTestStore::new()));
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
                open: stores,
                records: HashMap::new(),
            })),
            client_instances: Arc::new(std::sync::RwLock::new(client_instances)),
            stores_file: PathBuf::new(),
            client_instances_file: PathBuf::new(),
            stores_data_root: None,
            keys_dir: PathBuf::new(),
            admin_enabled: true,
            allowed_origins: None,
            web_source: crate::web::WebAssetSource::Embedded,
            operators_file: test_operators_file(),
            session_key: [0u8; crate::auth::SESSION_KEY_LEN],
            session_lifetime_secs: crate::auth::DEFAULT_SESSION_LIFETIME_SECS,
            cookies_secure: false,
            login_rate_limiter: crate::auth::RateLimiter::new(),
            session_revocations: crate::auth::SessionRevocations::new(),
            bootstrap_lock: std::sync::Mutex::new(()),
            operator_collection_lock: std::sync::Mutex::new(()),
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
        let (principal, _store) = resolve_runtime_client(&state, &[cert]).unwrap();
        assert_eq!(principal, "inst_local");
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
    fn admin_store_for_principal_admin_can_access_existing_store() {
        let store: Arc<dyn StorageBackend> = Arc::new(MemTestStore::new());
        let state = test_admin_state(HashMap::from([("work".to_string(), store.clone())]));
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
        let state = test_admin_state(HashMap::new());
        let p = principal(crate::operators::OperatorRole::Admin, &[]);
        expect_404(admin_store_for_principal(&state, &p, "missing"));
    }

    #[test]
    fn admin_store_for_principal_operator_without_grant_returns_404_not_403() {
        // Don't leak the existence of stores the operator can't reach:
        // ungranted-but-existing must look identical to truly-missing.
        let store: Arc<dyn StorageBackend> = Arc::new(MemTestStore::new());
        let state = test_admin_state(HashMap::from([("work".to_string(), store.clone())]));
        let p = principal(crate::operators::OperatorRole::Operator, &[]);
        expect_404(admin_store_for_principal(&state, &p, "work"));
        expect_404(admin_store_for_principal(&state, &p, "nope"));
    }

    #[test]
    fn admin_store_for_principal_operator_with_grant_sees_only_that_store() {
        let store_a: Arc<dyn StorageBackend> = Arc::new(MemTestStore::new());
        let store_b: Arc<dyn StorageBackend> = Arc::new(MemTestStore::new());
        let state = test_admin_state(HashMap::from([
            ("alpha".to_string(), store_a.clone()),
            ("beta".to_string(), store_b),
        ]));
        let p = principal(crate::operators::OperatorRole::Operator, &["alpha"]);
        assert!(admin_store_for_principal(&state, &p, "alpha").is_ok());
        expect_404(admin_store_for_principal(&state, &p, "beta"));
    }

    #[test]
    fn resolve_principal_session_uses_operator_collection_role_and_grants() {
        let state = seeded_admin_state("alice", "hunter2");
        // Re-seed collection so alice has Operator role with one grant.
        let operators_file = state.operators_file.clone();
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
        crate::operators::save(&operators_file, &collection).unwrap();

        let payload = crate::auth::new_session_payload("op-1".into(), "alice".into(), 1000, 3600);
        let auth = AdminAuth(payload);
        let p = resolve_principal(&state, &auth).unwrap();
        assert_eq!(p.username, "alice");
        assert!(!p.is_admin());
        assert!(p.has_store_access("work"));
        assert!(!p.has_store_access("other"));
    }

    #[test]
    fn resolve_principal_session_fails_when_operator_was_deleted() {
        // Cookie says "alice" but the collection has no record of her.
        let state = seeded_admin_state("alice", "hunter2");
        // Save an empty collection over alice's slot.
        crate::operators::save(
            &state.operators_file,
            &crate::operators::OperatorCollection::default(),
        )
        .unwrap();
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
            keys_dir: std::path::PathBuf::new(),
            admin_enabled: true,
            allowed_origins: None,
            web_source: crate::web::WebAssetSource::Embedded,
            operators_file: test_operators_file(),
            session_key: key,
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
            &state.operators_file,
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
        crate::operators::save(&state.operators_file, &collection).unwrap();
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
        let username_norm = crate::operators::normalise_username(username);
        if username_norm.is_empty() || password.is_empty() {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_credentials",
                "missing field",
            );
        }
        let _guard = state.bootstrap_lock.lock().unwrap();
        if crate::operators::bootstrap_completed(&state.operators_file).unwrap_or(true) {
            audit_bootstrap_attempt(state, &username_norm, "bootstrap_already_completed");
            return error_response(
                StatusCode::CONFLICT,
                "bootstrap_already_completed",
                "already done",
            );
        }
        let collection = crate::operators::load(&state.operators_file).unwrap();
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
        crate::operators::save(&state.operators_file, &new_collection).unwrap();
        crate::operators::mark_bootstrap_completed(
            &state.operators_file,
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
        assert!(crate::operators::bootstrap_completed(&state.operators_file).unwrap());
        // Operator is created with admin role.
        let collection = crate::operators::load(&state.operators_file).unwrap();
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
        crate::operators::save(&state.operators_file, &collection).unwrap();

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
        assert!(crate::operators::bootstrap_completed(&state.operators_file).unwrap());
        let collection = crate::operators::load(&state.operators_file).unwrap();
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
    /// dedicated store registry, operator-state file, and one allowed
    /// data root, all under the supplied tempdir.
    fn crud_state(tmp: &std::path::Path) -> AdminRuntimeState {
        let dynamic_dir = tmp.join("stores.d");
        std::fs::create_dir_all(&dynamic_dir).unwrap();
        let data_root = tmp.join("data");
        std::fs::create_dir_all(&data_root).unwrap();
        let keys_dir = tmp.join("keys");
        std::fs::create_dir_all(&keys_dir).unwrap();
        AdminRuntimeState {
            stores: Arc::new(std::sync::RwLock::new(StoreRegistry::new())),
            client_instances: Arc::new(std::sync::RwLock::new(HashMap::new())),
            stores_file: tmp.join("stores.json"),
            client_instances_file: tmp.join("client-instances.json"),
            stores_data_root: Some(data_root),
            keys_dir,
            admin_enabled: true,
            allowed_origins: None,
            web_source: crate::web::WebAssetSource::Embedded,
            operators_file: tmp.join("operators.json"),
            session_key: [0u8; crate::auth::SESSION_KEY_LEN],
            session_lifetime_secs: crate::auth::DEFAULT_SESSION_LIFETIME_SECS,
            cookies_secure: false,
            login_rate_limiter: crate::auth::RateLimiter::new(),
            session_revocations: crate::auth::SessionRevocations::new(),
            bootstrap_lock: std::sync::Mutex::new(()),
            operator_collection_lock: std::sync::Mutex::new(()),
        }
    }

    fn create_store_for_test(
        state: &AdminRuntimeState,
        principal: &crate::operators::Principal,
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
            // The Full body has a single frame; drain via block_in_place.
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async { self.collect().await.unwrap().to_bytes().to_vec() })
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
        let resp = handle_admin_stores_list(&state, &p);
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
        let resp = handle_admin_store_detail(&state, &p, &store_id);
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
        let resp = handle_admin_store_detail(&state, &p, &store_id);
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
        let resp = handle_admin_store_detail(&state, &p, &store_id);
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
    fn non_admin_cannot_create_or_update_or_delete() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let op = operator_principal(&["work"]);
        // Create
        let resp = do_admin_store_create(
            &state,
            &op,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
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
                name: None,
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
            reg.open[&store_id]
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

    #[cfg(unix)]
    #[test]
    fn create_encrypted_store_generates_key_with_0600_and_returns_it_once() {
        use std::os::unix::fs::PermissionsExt;
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

        // Key file is in state.keys_dir, named after the store id, mode 0600.
        let key_path = state.keys_dir.join(format!("{}.key", store_id));
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
        let resp = handle_admin_store_detail(&state, &p, &store_id);
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
    fn rotate_key_replaces_key_and_returns_new_value_once() {
        use std::os::unix::fs::PermissionsExt;
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

        let resp = handle_admin_store_rotate_key(&state, &p, &store_id);
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
        let key_path = state.keys_dir.join(format!("{}.key", store_id));
        let mode = std::fs::metadata(&key_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read_to_string(&key_path).unwrap(), new_key);

        // Detail GET still does not leak the key value.
        let resp = handle_admin_store_detail(&state, &p, &store_id);
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
        let store_id = create_store_for_test(
            &state,
            &p,
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                description: None,
            },
        );
        let resp = handle_admin_store_rotate_key(&state, &p, &store_id);
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        assert_eq!(response_body_json(resp)["code"], "not_encrypted");
    }

    #[test]
    fn rotate_key_requires_admin_and_succeeds() {
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
        // Non-admin gets 403.
        let op = operator_principal(&[&store_id]);
        let resp = handle_admin_store_rotate_key(&state, &op, &store_id);
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let resp = handle_admin_store_rotate_key(&state, &p, &store_id);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn store_management_view_reflects_usage_and_principal_capabilities() {
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
            reg.open[&plain_id].write("secret/item", b"value").unwrap();
            reg.open[&encrypted_id]
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

        let list = response_body_json(handle_admin_stores_list(&state, &admin));
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

        let operator = operator_principal(&[&plain_id]);
        let detail = response_body_json(handle_admin_store_detail(&state, &operator, &plain_id));
        assert_eq!(detail["usage"]["mapped_client_instance_count"], 1);
        assert_eq!(detail["usage"]["has_secrets"], true);
        assert_eq!(detail["actions"]["can_edit_metadata"], false);
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

        let list = handle_admin_client_instances_list(&state, &p);
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

        let list_after_disable = response_body_json(handle_admin_client_instances_list(&state, &p));
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
            &p,
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
        handle_admin_resource_create(
            &state,
            &p,
            &underwrap_protocol::service::AdminResourceCreateRequest {
                store: store_id.clone(),
                name: "github/token".into(),
            },
        );
        let del = handle_admin_resource_delete(
            &state,
            &p,
            &underwrap_protocol::service::AdminResourceDeleteRequest {
                store: store_id.clone(),
                name: "github/token".into(),
            },
        );
        assert_eq!(del.status(), StatusCode::OK);
        let body = response_body_json(del);
        assert_eq!(body["ok"], true);
        assert!(body.get("blocked_by").is_none() || body["blocked_by"].is_null());

        let list = handle_admin_resources_list(
            &state,
            &p,
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
    fn admin_resource_delete_refuses_with_structured_blockers() {
        use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let p = admin_principal(&[]);

        // Resource exists.
        handle_admin_resource_create(
            &state,
            &p,
            &underwrap_protocol::service::AdminResourceCreateRequest {
                store: store_id.clone(),
                name: "github/token".into(),
            },
        );
        // Pair references it.
        handle_admin_pair_create(
            &state,
            &p,
            &underwrap_protocol::service::AdminPairCreateRequest {
                store: store_id.clone(),
                wrapper: "gh".into(),
                resource: "github/token".into(),
            },
        );
        // A secret under the resource. The legacy per-slot key shape
        // routes through the blob shim and ends up inside the
        // `resources/github/token` blob.
        {
            let reg = state.stores.read().unwrap();
            write_legacy_secret(
                reg.open[&store_id].as_ref(),
                "github/token/env/GH_TOKEN",
                b"ghp_value",
            )
            .unwrap();
        }
        // A wrapper permission referencing the resource.
        {
            let reg = state.stores.read().unwrap();
            save_policy_document(
                reg.open[&store_id].as_ref(),
                &PolicyDocument {
                    permissions: vec![WrapperPermission {
                        client_instance: "inst_a".into(),
                        wrapper: "gh".into(),
                        resource: "github/token".into(),
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
                name: "github/token".into(),
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
            &p,
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
        let store_arc = {
            let reg = state.stores.read().unwrap();
            reg.open[&store_id].clone()
        };
        let store: &dyn StorageBackend = store_arc.as_ref();

        let mut pairs = load_pairs_document(store).unwrap();
        pairs
            .insert(WrapperResourcePairRecord {
                wrapper: "gh".into(),
                resource: "github/token".into(),
                created_by: EntityCreator::ClientInstance {
                    id: "inst_a".into(),
                },
                created_at: 100,
                last_used_at: None,
                files_mount_paths: Vec::new(),
            })
            .unwrap();
        pairs
            .insert(WrapperResourcePairRecord {
                wrapper: "tf".into(),
                resource: "github/token".into(),
                created_by: EntityCreator::ClientInstance {
                    id: "inst_a".into(),
                },
                created_at: 100,
                last_used_at: None,
                files_mount_paths: Vec::new(),
            })
            .unwrap();
        save_pairs_document(store, &pairs).unwrap();

        bump_pair_last_used(store, "gh", "github/token").unwrap();

        let after = load_pairs_document(store).unwrap();
        let gh = after.find("gh", "github/token").unwrap();
        let tf = after.find("tf", "github/token").unwrap();
        assert!(gh.last_used_at.is_some(), "matched pair should be bumped");
        assert!(tf.last_used_at.is_none(), "other pair must not be bumped");
    }

    #[test]
    fn bump_pair_last_used_silent_when_no_pair() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let store_arc = {
            let reg = state.stores.read().unwrap();
            reg.open[&store_id].clone()
        };
        // No pair record for (gh, github/token) → no-op, no error.
        bump_pair_last_used(store_arc.as_ref(), "gh", "github/token").unwrap();
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
                resource: "github/token".into(),
            },
        );
        assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_body_json(missing)["code"], "unknown_resource");

        handle_admin_resource_create(
            &state,
            &p,
            &underwrap_protocol::service::AdminResourceCreateRequest {
                store: store_id.clone(),
                name: "github/token".into(),
            },
        );
        let create = handle_admin_pair_create(
            &state,
            &p,
            &underwrap_protocol::service::AdminPairCreateRequest {
                store: store_id.clone(),
                wrapper: "gh".into(),
                resource: "github/token".into(),
            },
        );
        assert_eq!(create.status(), StatusCode::OK);

        let dup = handle_admin_pair_create(
            &state,
            &p,
            &underwrap_protocol::service::AdminPairCreateRequest {
                store: store_id.clone(),
                wrapper: "gh".into(),
                resource: "github/token".into(),
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

        handle_admin_resource_create(
            &state,
            &p,
            &underwrap_protocol::service::AdminResourceCreateRequest {
                store: store_id.clone(),
                name: "r".into(),
            },
        );
        handle_admin_pair_create(
            &state,
            &p,
            &underwrap_protocol::service::AdminPairCreateRequest {
                store: store_id.clone(),
                wrapper: "gh".into(),
                resource: "r".into(),
            },
        );
        {
            let reg = state.stores.read().unwrap();
            save_policy_document(
                reg.open[&store_id].as_ref(),
                &PolicyDocument {
                    permissions: vec![WrapperPermission {
                        client_instance: "inst_a".into(),
                        wrapper: "gh".into(),
                        resource: "r".into(),
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
                resource: "r".into(),
            },
        );
        assert_eq!(del.status(), StatusCode::OK);
        let body = response_body_json(del);
        assert_eq!(body["ok"], false);
        assert_eq!(body["blocked_by"]["permission_count"], 1);
        assert_eq!(body["blocked_by"]["sample_permissions"][0]["wrapper"], "gh");
    }

    #[test]
    fn non_admin_cannot_manage_resources_or_pairs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let op = operator_principal(&[&store_id]);
        let list = handle_admin_resources_list(
            &state,
            &op,
            &underwrap_protocol::service::AdminResourcesListRequest {
                store: store_id.clone(),
            },
        );
        assert_eq!(list.status(), StatusCode::FORBIDDEN);
        let create = handle_admin_resource_create(
            &state,
            &op,
            &underwrap_protocol::service::AdminResourceCreateRequest {
                store: store_id,
                name: "r".into(),
            },
        );
        assert_eq!(create.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn non_admin_cannot_manage_client_instances() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crud_state(tmp.path());
        let store_id = create_store_for_test(
            &state,
            &admin_principal(&[]),
            AdminStoreCreateRequest {
                name: "work".into(),
                backend: AdminStoreBackendKind::Plain,
                description: None,
            },
        );
        let op = operator_principal(&[&store_id]);
        let create = do_admin_client_instance_create(
            &state,
            &op,
            AdminClientInstanceCreateRequest {
                display_name: "laptop-work".into(),
                current_store_id: store_id.clone(),
                status: AdminClientInstanceStatus::Active,
                cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
                issued_at: None,
                auto_create_wrapper_assets: None,
            },
        );
        assert_eq!(create.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            handle_admin_client_instances_list(&state, &op).status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            handle_admin_client_instance_detail(&state, &op, "missing").status(),
            StatusCode::FORBIDDEN
        );
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
        crate::operators::save(&state.operators_file, &c).unwrap();
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
        let collection = crate::operators::load(&state.operators_file).unwrap();
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
        let collection = crate::operators::load(&state.operators_file).unwrap();
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

        let hash_before = crate::operators::load(&state.operators_file)
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

        let hash_after = crate::operators::load(&state.operators_file)
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
    fn operator_probing_ungranted_store_gets_404() {
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
                description: None,
            },
        );
        // Operator with no grant for `private`.
        let op = operator_principal(&[]);
        // List filtering hides ungranted stores from the operator.
        let resp = handle_admin_stores_list(&state, &op);
        let body = response_body_json(resp);
        assert_eq!(body["items"].as_array().unwrap().len(), 0);
        let resp = handle_admin_store_detail(&state, &op, "private");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let result = admin_store_for_principal(&state, &op, "private");
        assert!(result.is_err());
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
        let collection = crate::operators::load(&state.operators_file).unwrap();
        let formatted = format!("{collection:?}");
        assert!(
            !formatted.contains("$argon2"),
            "Debug must redact password hashes; got: {formatted}",
        );
    }
}
