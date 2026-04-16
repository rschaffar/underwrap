//! HTTPS backend client for backend service mode.

use anyhow::{Context, Result, bail};
use base64::Engine;
use reqwest::Certificate;
use reqwest::Identity;
use reqwest::header::CONTENT_TYPE;
use std::fs;
use std::sync::mpsc;

use underwrap_protocol::config::BackendSocketConfig;
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::service::{
    ServiceDeleteRequest, ServiceDeleteResponse, ServiceErrorResponse, ServiceListRequest,
    ServiceListResponse, ServiceReadRequest, ServiceReadResponse, ServiceStatusResponse,
    ServiceWriteRequest, ServiceWriteResponse,
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
    pub fn open(config: &BackendSocketConfig) -> Result<Self> {
        let base_url = config
            .url
            .clone()
            .context("backend service mode requires backend.url")?;
        let ca_path = config
            .ca_file
            .as_ref()
            .context("backend service mode requires backend.ca_file")?;
        let cert_path = config
            .client_cert_file
            .as_ref()
            .context("backend service mode requires backend.client_cert_file")?;
        let key_path = config
            .client_key_file
            .as_ref()
            .context("backend service mode requires backend.client_key_file")?;

        let ca_pem = fs::read(ca_path)
            .with_context(|| format!("reading backend ca file {}", ca_path.display()))?;
        let cert_pem = fs::read(cert_path)
            .with_context(|| format!("reading backend client cert file {}", cert_path.display()))?;
        let key_pem = fs::read(key_path)
            .with_context(|| format!("reading backend client key file {}", key_path.display()))?;
        let mut identity_pem = cert_pem;
        identity_pem.extend_from_slice(&key_pem);

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("building service backend runtime");
            let client = reqwest::Client::builder()
                .use_rustls_tls()
                .add_root_certificate(
                    Certificate::from_pem(&ca_pem).expect("parsing backend CA PEM"),
                )
                .identity(Identity::from_pem(&identity_pem).expect("parsing backend identity PEM"))
                .build()
                .expect("building backend HTTPS client");

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
