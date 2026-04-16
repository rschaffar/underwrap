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
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tracing::{error, info};

use underwrap_protocol::config::StorageConfig;
use underwrap_protocol::policy::POLICY_DOCUMENT_SECRET_ID;

#[cfg(test)]
use underwrap_protocol::message::{BackendErrorCode, BackendRequest, BackendResponse};
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
pub struct AdminConfig {
    pub listen: String,
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
        let admin_listener = TcpListener::bind(&admin_config.listen)
            .await
            .with_context(|| format!("binding admin listener {}", admin_config.listen))?;
        let admin_state = Arc::new(AdminRuntimeState {
            stores: stores.clone(),
        });
        info!(listen = %admin_config.listen, stores = admin_state.stores.len(), "backend admin HTTP listening");
        tasks.spawn(admin_accept_loop(admin_listener, admin_state));
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

#[cfg(test)]
async fn process_request(request: &BackendRequest, store: &dyn StorageBackend) -> BackendResponse {
    match request {
        BackendRequest::Read { secret_id } => match store.read(secret_id) {
            Ok(material) => {
                info!(secret = %secret_id, present = material.is_some(), "backend read");
                BackendResponse::ReadOk { material }
            }
            Err(e) => BackendResponse::Error {
                code: BackendErrorCode::Internal,
                message: format!("storage error: {e}"),
            },
        },
        BackendRequest::Write {
            secret_id,
            material,
        } => match store.write(secret_id, material.as_ref()) {
            Ok(()) => {
                info!(secret = %secret_id, "backend write");
                BackendResponse::WriteOk
            }
            Err(e) => BackendResponse::Error {
                code: BackendErrorCode::Internal,
                message: format!("storage error: {e}"),
            },
        },
        BackendRequest::List { prefix } => match store.list(prefix) {
            Ok(ids) => {
                info!(prefix = %prefix, count = ids.len(), "backend list");
                BackendResponse::ListOk { ids }
            }
            Err(e) => BackendResponse::Error {
                code: BackendErrorCode::Internal,
                message: format!("storage error: {e}"),
            },
        },
        BackendRequest::Delete { secret_id } => match store.delete(secret_id) {
            Ok(existed) => {
                info!(secret = %secret_id, existed = existed, "backend delete");
                BackendResponse::DeleteOk { existed }
            }
            Err(e) => BackendResponse::Error {
                code: BackendErrorCode::Internal,
                message: format!("storage error: {e}"),
            },
        },
        BackendRequest::Status => {
            info!("backend status");
            BackendResponse::StatusOk
        }
    }
}

async fn admin_accept_loop(listener: TcpListener, state: Arc<AdminRuntimeState>) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_admin_connection(stream, state).await {
                error!("backend admin connection error: {e:#}");
            }
        });
    }
}

async fn handle_admin_connection(
    stream: tokio::net::TcpStream,
    state: Arc<AdminRuntimeState>,
) -> Result<()> {
    let service = service_fn(move |req| {
        let state = Arc::clone(&state);
        async move { handle_admin_request(req, state).await }
    });

    hyper::server::conn::http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service)
        .await
        .context("serving admin HTTP connection")?;
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

fn admin_store<'a>(
    state: &'a AdminRuntimeState,
    store: &str,
) -> Result<&'a Arc<dyn StorageBackend>> {
    state
        .stores
        .get(store)
        .with_context(|| format!("unknown store '{store}'"))
}

async fn handle_admin_request(
    request: Request<Incoming>,
    state: Arc<AdminRuntimeState>,
) -> std::result::Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let response = match (request.method(), request.uri().path()) {
        (&Method::GET, "/v1/admin/stores") => {
            let mut stores: Vec<String> = state.stores.keys().cloned().collect();
            stores.sort();
            json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminStoresResponse { stores },
            )
        }
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
                Ok(body) => match base64::engine::general_purpose::STANDARD
                    .decode(&body.material_b64)
                {
                    Ok(material) => match admin_store(&state, &body.store)
                        .and_then(|store| store.write(&body.secret_id, &material))
                    {
                        Ok(()) => json_response(
                            StatusCode::OK,
                            &underwrap_protocol::service::AdminOkResponse { ok: true },
                        ),
                        Err(e) => {
                            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
                        }
                    },
                    Err(e) => error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        &format!("invalid base64 payload: {e}"),
                    ),
                },
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
        (&Method::POST, "/v1/admin/wrapper/set-state") => match parse_json_body::<
            underwrap_protocol::service::AdminWrapperStateRequest,
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
                    state.enabled = body.enabled;
                    state.reason = body.reason.clone();
                } else {
                    doc.wrappers
                        .push(underwrap_protocol::policy::WrapperPolicyState {
                            wrapper: body.wrapper.clone(),
                            enabled: body.enabled,
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

        fn with_data(entries: &[(&str, &[u8])]) -> Self {
            let store = Self::new();
            let mut data = store.data.lock().unwrap();
            for (key, value) in entries {
                data.insert((*key).to_string(), value.to_vec());
            }
            drop(data);
            store
        }

        fn fail(&self, op: &'static str) {
            self.fail_ops.lock().unwrap().insert(op);
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

    #[tokio::test]
    async fn process_request_read_write_list_delete_and_status() {
        let store = TestStore::with_data(&[("github/token", b"ghp_test")]);

        let resp = process_request(
            &BackendRequest::Read {
                secret_id: "github/token".to_string(),
            },
            &store,
        )
        .await;
        assert_eq!(
            resp,
            BackendResponse::ReadOk {
                material: Some(b"ghp_test".as_slice().into())
            }
        );

        let resp = process_request(
            &BackendRequest::Write {
                secret_id: "github/user".to_string(),
                material: b"octocat".as_slice().into(),
            },
            &store,
        )
        .await;
        assert_eq!(resp, BackendResponse::WriteOk);

        let resp = process_request(
            &BackendRequest::List {
                prefix: "github".to_string(),
            },
            &store,
        )
        .await;
        assert_eq!(
            resp,
            BackendResponse::ListOk {
                ids: vec!["github/token".to_string(), "github/user".to_string()]
            }
        );

        let resp = process_request(
            &BackendRequest::Delete {
                secret_id: "github/token".to_string(),
            },
            &store,
        )
        .await;
        assert_eq!(resp, BackendResponse::DeleteOk { existed: true });

        let resp = process_request(&BackendRequest::Status, &store).await;
        assert_eq!(resp, BackendResponse::StatusOk);
    }

    #[tokio::test]
    async fn process_request_maps_storage_errors() {
        for (op, request) in [
            (
                "read",
                BackendRequest::Read {
                    secret_id: "github/token".to_string(),
                },
            ),
            (
                "write",
                BackendRequest::Write {
                    secret_id: "github/token".to_string(),
                    material: b"value".as_slice().into(),
                },
            ),
            (
                "list",
                BackendRequest::List {
                    prefix: "github".to_string(),
                },
            ),
            (
                "delete",
                BackendRequest::Delete {
                    secret_id: "github/token".to_string(),
                },
            ),
        ] {
            let store = TestStore::new();
            store.fail(op);

            let resp = process_request(&request, &store).await;
            match resp {
                BackendResponse::Error { code, message } => {
                    assert_eq!(code, BackendErrorCode::Internal);
                    assert!(message.contains("storage error"));
                    assert!(message.contains(op));
                }
                other => panic!("expected Error, got {other:?}"),
            }
        }
    }

    #[test]
    fn policy_document_roundtrips_through_management_helpers() {
        let stores = HashMap::from([(
            "work".to_string(),
            Arc::new(TestStore::new()) as Arc<dyn StorageBackend>,
        )]);

        let json = r#"{"wrappers":[{"wrapper":"gh-comment","enabled":true}],"rules":[{"id":"r1","wrapper":"gh-comment","resource":"secret:github/token","operation":"secret.read","effect":"allow"}]}"#;
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
}
