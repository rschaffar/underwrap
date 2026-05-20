//! HTTPS backend client for backend service mode.

use anyhow::{Context, Result, bail};
use base64::Engine;
use reqwest::Certificate;
use reqwest::Identity;
use reqwest::header::CONTENT_TYPE;
use std::collections::HashMap;
use std::fs;
use std::sync::mpsc;

use underwrap_protocol::config::BackendServiceConfig;
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::service::{
    ServiceDeleteRequest, ServiceDeleteResponse, ServiceErrorResponse, ServiceListRequest,
    ServiceListResponse, ServiceReadPrefixesRequest, ServiceReadPrefixesResponse,
    ServiceReadRequest, ServiceReadResponse, ServiceStatusResponse, ServiceWrapperRegisterRequest,
    ServiceWrapperRegisterResponse, ServiceWriteRequest, ServiceWriteResponse,
};

use crate::StorageBackend;

enum Command {
    Read {
        secret_id: String,
        reply: mpsc::Sender<Result<Option<SecretBytes>>>,
    },
    Write {
        secret_id: String,
        material: Vec<u8>,
        reply: mpsc::Sender<Result<()>>,
    },
    List {
        prefix: String,
        reply: mpsc::Sender<Result<Vec<String>>>,
    },
    ReadPrefixes {
        prefixes: Vec<String>,
        reply: mpsc::Sender<Result<HashMap<String, SecretBytes>>>,
    },
    RegisterWrapperAssets {
        request: ServiceWrapperRegisterRequest,
        reply: mpsc::Sender<Result<ServiceWrapperRegisterResponse>>,
    },
    Delete {
        secret_id: String,
        reply: mpsc::Sender<Result<bool>>,
    },
    Status {
        reply: mpsc::Sender<Result<ServiceStatusResponse>>,
    },
}

pub struct ServiceBackend {
    sender: mpsc::Sender<Command>,
}

impl ServiceBackend {
    pub fn open(config: &BackendServiceConfig) -> Result<Self> {
        let base_url = config.url.clone();
        let cert_path = config
            .client_cert_file
            .as_ref()
            .context("backend requires client_cert_file")?;
        let key_path = config
            .client_key_file
            .as_ref()
            .context("backend requires client_key_file")?;

        let cert_pem = fs::read(cert_path)
            .with_context(|| format!("reading backend client cert file {}", cert_path.display()))?;
        let key_pem = fs::read(key_path)
            .with_context(|| format!("reading backend client key file {}", key_path.display()))?;
        let mut identity_pem = cert_pem;
        identity_pem.extend_from_slice(&key_pem);

        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut builder = reqwest::Client::builder()
            .use_rustls_tls()
            .identity(Identity::from_pem(&identity_pem).context("parsing backend identity PEM")?);
        if let Some(ca_path) = config.ca_file.as_ref() {
            let ca_pem = fs::read(ca_path)
                .with_context(|| format!("reading backend ca file {}", ca_path.display()))?;
            builder = builder.add_root_certificate(
                Certificate::from_pem(&ca_pem).context("parsing backend CA PEM")?,
            );
        }
        let client = builder.build().context("building backend HTTPS client")?;

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("building service backend runtime");

            for command in rx {
                match command {
                    Command::Read { secret_id, reply } => {
                        let _ = reply
                            .send(runtime.block_on(read_request(&client, &base_url, &secret_id)));
                    }
                    Command::Write {
                        secret_id,
                        material,
                        reply,
                    } => {
                        let _ = reply
                            .send(runtime.block_on(write_request(
                                &client, &base_url, &secret_id, &material,
                            )));
                    }
                    Command::List { prefix, reply } => {
                        let _ =
                            reply.send(runtime.block_on(list_request(&client, &base_url, &prefix)));
                    }
                    Command::ReadPrefixes { prefixes, reply } => {
                        let _ =
                            reply
                                .send(runtime.block_on(read_prefixes_request(
                                    &client, &base_url, &prefixes,
                                )));
                    }
                    Command::RegisterWrapperAssets { request, reply } => {
                        let _ = reply.send(runtime.block_on(register_wrapper_assets_request(
                            &client, &base_url, &request,
                        )));
                    }
                    Command::Delete { secret_id, reply } => {
                        let _ = reply
                            .send(runtime.block_on(delete_request(&client, &base_url, &secret_id)));
                    }
                    Command::Status { reply } => {
                        let _ = reply.send(runtime.block_on(status_request(&client, &base_url)));
                    }
                }
            }
        });

        Ok(Self { sender: tx })
    }
}

async fn read_request(
    client: &reqwest::Client,
    base_url: &str,
    secret_id: &str,
) -> Result<Option<SecretBytes>> {
    let response = client
        .post(endpoint(base_url, "/v1/read"))
        .header(CONTENT_TYPE, "application/json")
        .json(&ServiceReadRequest {
            secret_id: secret_id.to_string(),
        })
        .send()
        .await
        .with_context(|| format!("sending backend read request for {secret_id}"))?;
    if !response.status().is_success() {
        bail!(parse_error(response).await);
    }
    let body: ServiceReadResponse = response
        .json()
        .await
        .context("decoding backend read response")?;
    Ok(body
        .material_b64
        .map(|material| base64::engine::general_purpose::STANDARD.decode(material))
        .transpose()
        .context("decoding backend read payload")?
        .map(Into::into))
}

async fn write_request(
    client: &reqwest::Client,
    base_url: &str,
    secret_id: &str,
    material: &[u8],
) -> Result<()> {
    let response = client
        .post(endpoint(base_url, "/v1/write"))
        .header(CONTENT_TYPE, "application/json")
        .json(&ServiceWriteRequest {
            secret_id: secret_id.to_string(),
            material_b64: base64::engine::general_purpose::STANDARD.encode(material),
        })
        .send()
        .await
        .with_context(|| format!("sending backend write request for {secret_id}"))?;
    if !response.status().is_success() {
        bail!(parse_error(response).await);
    }
    let _: ServiceWriteResponse = response
        .json()
        .await
        .context("decoding backend write response")?;
    Ok(())
}

async fn list_request(
    client: &reqwest::Client,
    base_url: &str,
    prefix: &str,
) -> Result<Vec<String>> {
    let response = client
        .post(endpoint(base_url, "/v1/list"))
        .header(CONTENT_TYPE, "application/json")
        .json(&ServiceListRequest {
            prefix: prefix.to_string(),
        })
        .send()
        .await
        .with_context(|| format!("sending backend list request for {prefix}"))?;
    if !response.status().is_success() {
        bail!(parse_error(response).await);
    }
    let body: ServiceListResponse = response
        .json()
        .await
        .context("decoding backend list response")?;
    Ok(body.ids)
}

async fn read_prefixes_request(
    client: &reqwest::Client,
    base_url: &str,
    prefixes: &[String],
) -> Result<HashMap<String, SecretBytes>> {
    let response = client
        .post(endpoint(base_url, "/v1/read-prefixes"))
        .header(CONTENT_TYPE, "application/json")
        .json(&ServiceReadPrefixesRequest {
            prefixes: prefixes.to_vec(),
        })
        .send()
        .await
        .context("sending backend read-prefixes request")?;
    if !response.status().is_success() {
        bail!(parse_error(response).await);
    }
    let body: ServiceReadPrefixesResponse = response
        .json()
        .await
        .context("decoding backend read-prefixes response")?;
    let mut out = HashMap::new();
    for entry in body.entries {
        let material = base64::engine::general_purpose::STANDARD
            .decode(entry.material_b64)
            .with_context(|| format!("decoding backend bundle payload for {}", entry.id))?;
        out.insert(entry.id, material.into());
    }
    Ok(out)
}

async fn register_wrapper_assets_request(
    client: &reqwest::Client,
    base_url: &str,
    request: &ServiceWrapperRegisterRequest,
) -> Result<ServiceWrapperRegisterResponse> {
    let response = client
        .post(endpoint(base_url, "/v1/runtime/wrappers/register"))
        .header(CONTENT_TYPE, "application/json")
        .json(request)
        .send()
        .await
        .context("sending backend wrapper register request")?;
    if !response.status().is_success() {
        bail!(parse_error(response).await);
    }
    response
        .json::<ServiceWrapperRegisterResponse>()
        .await
        .context("decoding backend wrapper register response")
}

async fn delete_request(client: &reqwest::Client, base_url: &str, secret_id: &str) -> Result<bool> {
    let response = client
        .post(endpoint(base_url, "/v1/delete"))
        .header(CONTENT_TYPE, "application/json")
        .json(&ServiceDeleteRequest {
            secret_id: secret_id.to_string(),
        })
        .send()
        .await
        .with_context(|| format!("sending backend delete request for {secret_id}"))?;
    if !response.status().is_success() {
        bail!(parse_error(response).await);
    }
    let body: ServiceDeleteResponse = response
        .json()
        .await
        .context("decoding backend delete response")?;
    Ok(body.existed)
}

async fn status_request(client: &reqwest::Client, base_url: &str) -> Result<ServiceStatusResponse> {
    let response = client
        .get(endpoint(base_url, "/v1/status"))
        .send()
        .await
        .context("sending backend status request")?;
    if !response.status().is_success() {
        bail!(parse_error(response).await);
    }
    response
        .json::<ServiceStatusResponse>()
        .await
        .context("decoding backend status response")
}

fn endpoint(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

async fn parse_error(response: reqwest::Response) -> anyhow::Error {
    let status = response.status();
    let text = response.text().await.unwrap_or_else(|_| String::new());
    if let Ok(error) = serde_json::from_str::<ServiceErrorResponse>(&text) {
        if error.code == "store_locked" {
            return anyhow::Error::new(ServiceStoreLocked {
                message: error.message,
            });
        }
        anyhow::anyhow!(
            "backend service error {} ({}): {}",
            status,
            error.code,
            error.message
        )
    } else {
        anyhow::anyhow!("backend service error {}: {}", status, text)
    }
}

/// Typed error variant that the frontend daemon can downcast to in
/// order to translate a backend `store_locked` HTTP error into the
/// `ErrorCode::StoreLocked` wrapper response. Carrying it through
/// `anyhow::Error::new(...)` keeps the rest of the storage error
/// surface unchanged while preserving structured propagation for
/// this one case.
#[derive(Debug)]
pub struct ServiceStoreLocked {
    pub message: String,
}

impl std::fmt::Display for ServiceStoreLocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "backend store is locked: {}", self.message)
    }
}

impl std::error::Error for ServiceStoreLocked {}

impl StorageBackend for ServiceBackend {
    fn read(&self, secret_id: &str) -> Result<Option<SecretBytes>> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Command::Read {
                secret_id: secret_id.to_string(),
                reply: tx,
            })
            .context("sending read command to service backend worker")?;
        rx.recv()
            .context("receiving read response from service backend worker")?
    }

    fn write(&self, secret_id: &str, material: &[u8]) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Command::Write {
                secret_id: secret_id.to_string(),
                material: material.to_vec(),
                reply: tx,
            })
            .context("sending write command to service backend worker")?;
        rx.recv()
            .context("receiving write response from service backend worker")?
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Command::List {
                prefix: prefix.to_string(),
                reply: tx,
            })
            .context("sending list command to service backend worker")?;
        rx.recv()
            .context("receiving list response from service backend worker")?
    }

    fn delete(&self, secret_id: &str) -> Result<bool> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Command::Delete {
                secret_id: secret_id.to_string(),
                reply: tx,
            })
            .context("sending delete command to service backend worker")?;
        rx.recv()
            .context("receiving delete response from service backend worker")?
    }

    fn read_prefixes(&self, prefixes: &[String]) -> Result<HashMap<String, SecretBytes>> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Command::ReadPrefixes {
                prefixes: prefixes.to_vec(),
                reply: tx,
            })
            .context("sending read-prefixes command to service backend worker")?;
        rx.recv()
            .context("receiving read-prefixes response from service backend worker")?
    }

    fn register_wrapper_assets(
        &self,
        request: &ServiceWrapperRegisterRequest,
    ) -> Result<ServiceWrapperRegisterResponse> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Command::RegisterWrapperAssets {
                request: request.clone(),
                reply: tx,
            })
            .context("sending register-wrapper-assets command to service backend worker")?;
        rx.recv()
            .context("receiving register-wrapper-assets response from service backend worker")?
    }

    fn principal(&self) -> Result<String> {
        Ok(self.status_response()?.principal)
    }
}

impl ServiceBackend {
    pub fn is_available(&self) -> bool {
        self.status_response().map(|s| s.ok).unwrap_or(false)
    }

    fn status_response(&self) -> Result<ServiceStatusResponse> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Command::Status { reply: tx })
            .context("sending status command to service backend worker")?;
        rx.recv()
            .context("receiving status response from service backend worker")?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn open_fails_immediately_on_bad_ca() {
        let dir = tempfile::tempdir().unwrap();
        let ca = dir.path().join("ca.pem");
        let cert = dir.path().join("cert.pem");
        let key = dir.path().join("key.pem");
        std::fs::File::create(&ca)
            .unwrap()
            .write_all(b"not-a-cert")
            .unwrap();
        std::fs::File::create(&cert)
            .unwrap()
            .write_all(b"not-a-cert")
            .unwrap();
        std::fs::File::create(&key)
            .unwrap()
            .write_all(b"not-a-key")
            .unwrap();

        let config = BackendServiceConfig {
            url: "https://localhost:0".to_string(),
            ca_file: Some(ca),
            client_cert_file: Some(cert),
            client_key_file: Some(key),
        };
        let result = ServiceBackend::open(&config);
        assert!(result.is_err(), "should fail on bad CA");
        let err = result.err().unwrap();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("parsing backend CA PEM") || msg.contains("parsing backend identity PEM"),
            "unexpected error: {msg}"
        );
    }
}
