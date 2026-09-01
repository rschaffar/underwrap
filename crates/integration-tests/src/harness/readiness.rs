//! Readiness probes: port picking and wait-for-URL/path helpers.

use anyhow::{Context, Result};
use std::path::Path;

fn admin_client(url_root_ca: &Path, timeout: std::time::Duration) -> Result<reqwest::Client> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ca_pem = std::fs::read(url_root_ca)?;
    reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(timeout)
        .add_root_certificate(reqwest::Certificate::from_pem(&ca_pem)?)
        .build()
        .map_err(Into::into)
}

fn parse_cookie_value(set_cookie: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    set_cookie
        .strip_prefix(&prefix)
        .and_then(|rest| rest.split(';').next())
        .map(str::to_string)
}

/// Compose the `Cookie:` header used by admin-API requests: the
/// opaque `tower-sessions` session id plus the random `axum_csrf`
/// token-cookie. The cookie holds the *raw* token; the
/// `X-CSRF-Token` request header carries the HMAC authenticity token
/// derived from it.
pub(crate) fn csrf_cookie_header(session_cookie: &str, csrf_cookie: &str) -> String {
    format!("__Host-underwrap-session={session_cookie}; __Host-underwrap-csrf={csrf_cookie}")
}

/// Material the harness needs to authenticate subsequent admin
/// requests after a successful login: the session
/// cookie value, the CSRF cookie value, and the HMAC authenticity
/// token to echo back in `X-CSRF-Token`.
pub(crate) struct AdminCredentials {
    pub session_cookie: String,
    pub csrf_cookie: String,
    pub csrf_token: String,
}

pub(crate) async fn login_admin(
    url: &str,
    ca_cert: &Path,
    username: &str,
    password: &str,
) -> Result<AdminCredentials> {
    let client = admin_client(ca_cert, std::time::Duration::from_secs(5))?;
    let response = client
        .post(format!("{url}/v1/admin/auth/login"))
        .json(&serde_json::json!({
            "username": username,
            "password": password,
        }))
        .send()
        .await?;
    anyhow::ensure!(
        response.status().is_success(),
        "login returned {}",
        response.status()
    );
    let csrf_token = response
        .headers()
        .get("X-CSRF-Token")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .context("login response missing X-CSRF-Token header")?;
    let cookies = response.headers().get_all(reqwest::header::SET_COOKIE);
    let mut session_cookie = None;
    let mut csrf_cookie = None;
    for value in cookies.iter().filter_map(|v| v.to_str().ok()) {
        if session_cookie.is_none() {
            session_cookie = parse_cookie_value(value, "__Host-underwrap-session");
        }
        if csrf_cookie.is_none() {
            csrf_cookie = parse_cookie_value(value, "__Host-underwrap-csrf");
        }
    }
    let session_cookie = session_cookie.context("login response missing session cookie")?;
    let csrf_cookie = csrf_cookie.context("login response missing csrf cookie")?;
    Ok(AdminCredentials {
        session_cookie,
        csrf_cookie,
        csrf_token,
    })
}

fn backend_port_startup_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// A process-local reservation for one backend fixture's release-to-bind window.
///
/// Keep this value alive until an HTTPS readiness probe proves that the fixture
/// server owns the selected port.
pub(crate) struct BackendPortReservation {
    port: u16,
    _startup_guard: tokio::sync::MutexGuard<'static, ()>,
}

impl BackendPortReservation {
    pub(crate) fn port(&self) -> u16 {
        self.port
    }
}

/// Ask the kernel for an unused TCP port while excluding every other backend
/// fixture in this test process from selecting a port until this reservation is
/// dropped.
pub(crate) async fn reserve_backend_port() -> Result<BackendPortReservation> {
    let startup_guard = backend_port_startup_lock().lock().await;
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").context("binding ephemeral port for harness")?;
    let port = listener
        .local_addr()
        .context("reading local_addr from ephemeral listener")?
        .port();
    drop(listener);
    Ok(BackendPortReservation {
        port,
        _startup_guard: startup_guard,
    })
}

// Total window: 3000 × 20 ms ≈ 60 s. The loop exits as soon as the listener
// answers, while the generous ceiling accommodates CPU and I/O contention when
// `cargo test --workspace` starts many fixtures in parallel.
pub(crate) async fn wait_for_admin(url: &str, ca_cert: &Path) -> Result<()> {
    let client = admin_client(ca_cert, std::time::Duration::from_millis(500))?;
    for _ in 0..3000 {
        if let Ok(response) = client
            .get(format!("{url}/v1/admin/auth/bootstrap/status"))
            .send()
            .await
            && response.status().is_success()
        {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    anyhow::bail!("admin API never became ready at {url}")
}

// Total window: 3000 × 20 ms ≈ 60 s. See wait_for_admin above; rustls
// initialisation is the slowest part of backend startup under load.
pub(crate) async fn wait_for_service(
    url: &str,
    ca_cert: &Path,
    client_identity: &Path,
) -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ca_pem = std::fs::read(ca_cert)?;
    let identity_pem = std::fs::read(client_identity)?;
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(std::time::Duration::from_millis(500))
        .add_root_certificate(reqwest::Certificate::from_pem(&ca_pem)?)
        .identity(reqwest::Identity::from_pem(&identity_pem)?)
        .build()?;

    // Probe the new resource-vocabulary runtime API. An empty
    // wrappers register is a stateless round-trip that confirms the
    // backend is listening AND the cert is authorized.
    let probe = underwrap_protocol::service::RuntimeWrapperRegisterRequest { wrappers: vec![] };
    for _ in 0..3000 {
        if let Ok(response) = client
            .post(format!("{url}/v1/wrappers"))
            .json(&probe)
            .send()
            .await
            && response.status().is_success()
        {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    anyhow::bail!("service never became ready at {url}")
}

pub(super) async fn wait_for_path(path: &Path) -> Result<()> {
    for _ in 0..100 {
        if path.exists() {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    anyhow::bail!("path never appeared: {}", path.display())
}

#[cfg(test)]
mod tests;
