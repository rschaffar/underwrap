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
    CatalogEntry, ServiceCatalogPutRequest, ServiceCatalogPutResponse, ServiceDeleteRequest,
    ServiceDeleteResponse, ServiceErrorResponse, ServiceListRequest, ServiceListResponse,
    ServiceReadPrefixesRequest, ServiceReadPrefixesResponse, ServiceReadRequest,
    ServiceReadResponse, ServiceStatusResponse, ServiceWriteRequest, ServiceWriteResponse,
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
    RegisterCatalog {
        entries: Vec<CatalogEntry>,
        reply: mpsc::Sender<Result<()>>,
    },
    Delete {
        secret_id: String,
        reply: mpsc::Sender<Result<bool>>,
    },
    Status {
        reply: mpsc::Sender<bool>,
    },
}

pub struct ServiceBackend {
    sender: mpsc::Sender<Command>,
}

impl ServiceBackend {
    pub fn open(config: &BackendServiceConfig) -> Result<Self> {
        let base_url = config.url.clone();
        let ca_path = config
            .ca_file
            .as_ref()
            .context("backend requires ca_file")?;
        let cert_path = config
            .client_cert_file
            .as_ref()
            .context("backend requires client_cert_file")?;
        let key_path = config
            .client_key_file
            .as_ref()
            .context("backend requires client_key_file")?;

        let ca_pem = fs::read(ca_path)
            .with_context(|| format!("reading backend ca file {}", ca_path.display()))?;
        let cert_pem = fs::read(cert_path)
            .with_context(|| format!("reading backend client cert file {}", cert_path.display()))?;
        let key_pem = fs::read(key_path)
            .with_context(|| format!("reading backend client key file {}", key_path.display()))?;
        let mut identity_pem = cert_pem;
        identity_pem.extend_from_slice(&key_pem);

        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .add_root_certificate(Certificate::from_pem(&ca_pem).context("parsing backend CA PEM")?)
            .identity(Identity::from_pem(&identity_pem).context("parsing backend identity PEM")?)
            .build()
            .context("building backend HTTPS client")?;

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
                    Command::RegisterCatalog { entries, reply } => {
                        let _ = reply.send(
                            runtime
                                .block_on(register_catalog_request(&client, &base_url, &entries)),
                        );
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

async fn register_catalog_request(
    client: &reqwest::Client,
    base_url: &str,
    entries: &[CatalogEntry],
) -> Result<()> {
    let response = client
        .post(endpoint(base_url, "/v1/catalog/put"))
        .header(CONTENT_TYPE, "application/json")
        .json(&ServiceCatalogPutRequest {
            entries: entries.to_vec(),
        })
        .send()
        .await
        .context("sending backend catalog registration request")?;
    if !response.status().is_success() {
        bail!(parse_error(response).await);
    }
    let _: ServiceCatalogPutResponse = response
        .json()
        .await
        .context("decoding backend catalog registration response")?;
    Ok(())
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

async fn status_request(client: &reqwest::Client, base_url: &str) -> bool {
    match client.get(endpoint(base_url, "/v1/status")).send().await {
        Ok(response) if response.status().is_success() => response
            .json::<ServiceStatusResponse>()
            .await
            .map(|status| status.ok)
            .unwrap_or(false),
        _ => false,
    }
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

    fn register_catalog(&self, entries: &[CatalogEntry]) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Command::RegisterCatalog {
                entries: entries.to_vec(),
                reply: tx,
            })
            .context("sending register-catalog command to service backend worker")?;
        rx.recv()
            .context("receiving register-catalog response from service backend worker")?
    }
}

impl ServiceBackend {
    pub fn is_available(&self) -> bool {
        let (tx, rx) = mpsc::channel();
        if self.sender.send(Command::Status { reply: tx }).is_err() {
            return false;
        }
        rx.recv().unwrap_or(false)
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
