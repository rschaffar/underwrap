//! Typed runtime client for the central `underwrap-backendd`.
//!
//! Speaks exactly three endpoints:
//!
//! - `POST /v1/wrappers` — register the daemon's `(wrapper, resource)`
//!   pairs at startup; the backend either auto-creates the missing
//!   entities or refuses with a structured payload.
//! - `POST /v1/wrapper-invocations` — open one invocation: returns the
//!   canonical resource blob plus a `writable` hint, or a structured
//!   `Deny`.
//! - `POST /v1/wrapper-invocations/finalize` — close one invocation,
//!   optionally writing a new blob. The backend independently
//!   re-checks writability and env-section immutability before
//!   persisting.
//!
//! The runtime client never names a storage key, never identifies
//! itself by id, and never evaluates policy. The mTLS identity bound
//! to the cert IS the principal; the backend re-derives it on every
//! call.

use anyhow::{Context, Result, bail};
use base64::Engine;
use reqwest::Certificate;
use reqwest::Identity;
use reqwest::header::CONTENT_TYPE;
use std::fs;
use std::sync::mpsc;

use underwrap_protocol::config::BackendServiceConfig;
use underwrap_protocol::service::{
    RuntimeWrapperInvocationFinalizeRequest, RuntimeWrapperInvocationFinalizeResponse,
    RuntimeWrapperInvocationOpenRequest, RuntimeWrapperInvocationOpenResponse,
    RuntimeWrapperRegisterRequest, RuntimeWrapperRegisterResponse, ServiceErrorResponse,
};
use underwrap_protocol::variant::WrapperVariant;

/// Typed error variant that the daemon downcasts to translate the
/// backend's `store_locked` HTTP error into the appropriate wrapper
/// response. Carrying it via `anyhow::Error::new(...)` keeps the rest
/// of the call surface unchanged.
#[derive(Debug)]
pub struct StoreLockedError {
    pub message: String,
}

impl std::fmt::Display for StoreLockedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "backend store is locked: {}", self.message)
    }
}

impl std::error::Error for StoreLockedError {}

/// The backend refused the mTLS client's request as unauthenticated —
/// usually because the cert is unknown, the client instance is
/// disabled, or its credential was rotated and this daemon still
/// presents the old one. The daemon translates this into the
/// wrapper-facing "this underwrap instance is not enrolled yet"
/// message, so the operator gets a direct link to the admin UI.
#[derive(Debug)]
pub struct UnauthorizedError {
    pub message: String,
}

impl std::fmt::Display for UnauthorizedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "backend refused this client's credential: {}",
            self.message
        )
    }
}

impl std::error::Error for UnauthorizedError {}

enum Command {
    Register {
        request: RuntimeWrapperRegisterRequest,
        reply: mpsc::Sender<Result<RuntimeWrapperRegisterResponse>>,
    },
    Open {
        request: RuntimeWrapperInvocationOpenRequest,
        reply: mpsc::Sender<Result<RuntimeWrapperInvocationOpenResponse>>,
    },
    Finalize {
        request: RuntimeWrapperInvocationFinalizeRequest,
        reply: mpsc::Sender<Result<RuntimeWrapperInvocationFinalizeResponse>>,
    },
}

pub struct BackendClient {
    sender: mpsc::Sender<Command>,
}

impl BackendClient {
    /// Build the mTLS HTTPS client and spawn the I/O worker thread.
    /// The worker owns its own tokio runtime so synchronous call sites
    /// in the daemon can issue requests from any thread.
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
        // Disable HTTP keep-alive: the daemon issues a small handful
        // of requests per wrapper invocation and at most a couple of
        // invocations per second. The cost of one mTLS handshake per
        // request is negligible compared to the cost of failing-open
        // a wrapper call because reqwest tried to reuse a connection
        // the backend silently dropped (e.g. after a restart). Without
        // this, the first invocation after `systemctl restart
        // underwrap-backendd` hits a half-dead pooled connection and
        // surfaces as `peer closed connection without sending TLS
        // close_notify`; with it, every call opens a fresh TLS
        // connection and the daemon survives backend restarts without
        // operator intervention.
        let mut builder = reqwest::Client::builder()
            .use_rustls_tls()
            .pool_max_idle_per_host(0)
            .identity(Identity::from_pem(&identity_pem).context("parsing backend identity PEM")?);
        if let Some(ca_path) = config.ca_file.as_ref() {
            let ca_pem = fs::read(ca_path)
                .with_context(|| format!("reading backend ca file {}", ca_path.display()))?;
            builder = builder.add_root_certificate(
                Certificate::from_pem(&ca_pem).context("parsing backend CA PEM")?,
            );
        }
        let client = builder.build().context("building backend HTTPS client")?;

        let (tx, rx) = mpsc::channel::<Command>();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("building backend client runtime");
            for command in rx {
                match command {
                    Command::Register { request, reply } => {
                        let _ = reply
                            .send(runtime.block_on(register_request(&client, &base_url, &request)));
                    }
                    Command::Open { request, reply } => {
                        let _ = reply
                            .send(runtime.block_on(open_request(&client, &base_url, &request)));
                    }
                    Command::Finalize { request, reply } => {
                        let _ = reply
                            .send(runtime.block_on(finalize_request(&client, &base_url, &request)));
                    }
                }
            }
        });

        Ok(Self { sender: tx })
    }

    pub fn register(
        &self,
        request: RuntimeWrapperRegisterRequest,
    ) -> Result<RuntimeWrapperRegisterResponse> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Command::Register { request, reply: tx })
            .context("sending register command to backend client worker")?;
        rx.recv()
            .context("receiving register response from backend client worker")?
    }

    pub fn open_invocation(
        &self,
        wrapper_name: &str,
        resource_name: &str,
        variant: WrapperVariant,
    ) -> Result<RuntimeWrapperInvocationOpenResponse> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Command::Open {
                request: RuntimeWrapperInvocationOpenRequest {
                    wrapper_name: wrapper_name.to_string(),
                    resource_name: resource_name.to_string(),
                    variant,
                },
                reply: tx,
            })
            .context("sending open command to backend client worker")?;
        rx.recv()
            .context("receiving open response from backend client worker")?
    }

    pub fn finalize_invocation(
        &self,
        wrapper_name: &str,
        resource_name: &str,
        variant: WrapperVariant,
        blob_bytes: Option<&[u8]>,
    ) -> Result<RuntimeWrapperInvocationFinalizeResponse> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Command::Finalize {
                request: RuntimeWrapperInvocationFinalizeRequest {
                    wrapper_name: wrapper_name.to_string(),
                    resource_name: resource_name.to_string(),
                    variant,
                    blob_b64: blob_bytes
                        .map(|b| base64::engine::general_purpose::STANDARD.encode(b)),
                },
                reply: tx,
            })
            .context("sending finalize command to backend client worker")?;
        rx.recv()
            .context("receiving finalize response from backend client worker")?
    }
}

async fn register_request(
    client: &reqwest::Client,
    base_url: &str,
    request: &RuntimeWrapperRegisterRequest,
) -> Result<RuntimeWrapperRegisterResponse> {
    let response = client
        .post(endpoint(base_url, "/v1/wrappers"))
        .header(CONTENT_TYPE, "application/json")
        .json(request)
        .send()
        .await
        .context("sending backend register request")?;
    if !response.status().is_success() {
        bail!(parse_error(response).await);
    }
    response
        .json::<RuntimeWrapperRegisterResponse>()
        .await
        .context("decoding backend register response")
}

async fn open_request(
    client: &reqwest::Client,
    base_url: &str,
    request: &RuntimeWrapperInvocationOpenRequest,
) -> Result<RuntimeWrapperInvocationOpenResponse> {
    let response = client
        .post(endpoint(base_url, "/v1/wrapper-invocations"))
        .header(CONTENT_TYPE, "application/json")
        .json(request)
        .send()
        .await
        .context("sending backend open request")?;
    if !response.status().is_success() {
        bail!(parse_error(response).await);
    }
    response
        .json::<RuntimeWrapperInvocationOpenResponse>()
        .await
        .context("decoding backend open response")
}

async fn finalize_request(
    client: &reqwest::Client,
    base_url: &str,
    request: &RuntimeWrapperInvocationFinalizeRequest,
) -> Result<RuntimeWrapperInvocationFinalizeResponse> {
    let response = client
        .post(endpoint(base_url, "/v1/wrapper-invocations/finalize"))
        .header(CONTENT_TYPE, "application/json")
        .json(request)
        .send()
        .await
        .context("sending backend finalize request")?;
    if !response.status().is_success() {
        bail!(parse_error(response).await);
    }
    response
        .json::<RuntimeWrapperInvocationFinalizeResponse>()
        .await
        .context("decoding backend finalize response")
}

fn endpoint(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/'),
    )
}

async fn parse_error(response: reqwest::Response) -> anyhow::Error {
    let status = response.status();
    let text = response.text().await.unwrap_or_else(|_| String::new());
    if let Ok(error) = serde_json::from_str::<ServiceErrorResponse>(&text) {
        if error.code == "store_locked" {
            return anyhow::Error::new(StoreLockedError {
                message: error.message,
            });
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return anyhow::Error::new(UnauthorizedError {
                message: error.message,
            });
        }
        anyhow::anyhow!(
            "backend service error {} ({}): {}",
            status,
            error.code,
            error.message
        )
    } else if status == reqwest::StatusCode::UNAUTHORIZED {
        anyhow::Error::new(UnauthorizedError { message: text })
    } else {
        anyhow::anyhow!("backend service error {}: {}", status, text)
    }
}

#[cfg(test)]
mod tests;
