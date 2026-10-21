//! Readiness probes: port picking and wait-for-URL/path helpers.

use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

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

fn backend_startup_lock_path() -> PathBuf {
    // SAFETY: geteuid takes no pointers, has no failure return, and only reads
    // the calling process's effective identity.
    let euid = unsafe { libc::geteuid() };
    std::env::temp_dir().join(format!("underwrap-integration-backend-startup-{euid}.lock"))
}

fn open_backend_startup_lock_file() -> Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(backend_startup_lock_path())
        .context("opening cross-process backend startup lock")
}

fn lock_backend_startup_across_processes() -> Result<File> {
    let lock_file = open_backend_startup_lock_file()?;
    loop {
        // SAFETY: lock_file owns a valid descriptor for the duration of this
        // call. LOCK_EX changes only its advisory-lock state.
        if unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX) } == 0 {
            return Ok(lock_file);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error).context("locking cross-process backend startup lock");
        }
    }
}

/// A cooperative reservation for one backend fixture's release-to-bind window.
///
/// The in-process mutex avoids blocking another async fixture on `flock`, while
/// the close-on-exec file lock serializes cooperating Cargo and nextest test
/// processes that share the same temporary directory and effective UID.
///
/// Keep this value alive until an HTTPS readiness probe proves that the fixture
/// server owns the selected port. An unrelated process that does not take this
/// advisory lock can still claim the released loopback port.
pub(crate) struct BackendStartupReservation {
    port: u16,
    _cross_process_guard: File,
    _process_guard: tokio::sync::MutexGuard<'static, ()>,
}

impl BackendStartupReservation {
    pub(crate) fn port(&self) -> u16 {
        self.port
    }
}

/// Ask the kernel for an unused TCP port while excluding every cooperating
/// backend fixture until this reservation is dropped.
pub(crate) async fn reserve_backend_startup() -> Result<BackendStartupReservation> {
    let process_guard = backend_port_startup_lock().lock().await;
    let cross_process_guard = lock_backend_startup_across_processes()?;
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").context("binding ephemeral port for harness")?;
    let port = listener
        .local_addr()
        .context("reading local_addr from ephemeral listener")?
        .port();
    drop(listener);
    Ok(BackendStartupReservation {
        port,
        _cross_process_guard: cross_process_guard,
        _process_guard: process_guard,
    })
}

const READINESS_BUDGET: std::time::Duration = std::time::Duration::from_secs(60);
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);
const PROBE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(20);

pub(crate) async fn wait_for_admin(
    url: &str,
    ca_cert: &Path,
    backend_alive: impl FnMut() -> Result<()>,
) -> Result<()> {
    wait_for_https(url, ca_cert, None, READINESS_BUDGET, backend_alive).await
}

pub(crate) async fn wait_for_service(
    url: &str,
    ca_cert: &Path,
    client_identity: &Path,
    backend_alive: impl FnMut() -> Result<()>,
) -> Result<()> {
    wait_for_https(
        url,
        ca_cert,
        Some(client_identity),
        READINESS_BUDGET,
        backend_alive,
    )
    .await
}

async fn wait_for_https(
    url: &str,
    ca_cert: &Path,
    client_identity: Option<&Path>,
    budget: std::time::Duration,
    mut backend_alive: impl FnMut() -> Result<()>,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + budget;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut builder = reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(PROBE_TIMEOUT)
        .add_root_certificate(reqwest::Certificate::from_pem(&std::fs::read(ca_cert)?)?);
    if let Some(identity) = client_identity {
        builder = builder.identity(reqwest::Identity::from_pem(&std::fs::read(identity)?)?);
    }
    let client = builder.build()?;
    let label = if client_identity.is_some() {
        "runtime"
    } else {
        "admin"
    };
    let mut last = "no completed probe".to_string();
    loop {
        backend_alive().context("backend exited during HTTPS readiness")?;
        let now = tokio::time::Instant::now();
        if now >= deadline {
            anyhow::bail!("{label} API not ready at {url} within {budget:?}; last probe: {last}");
        }
        let request = if client_identity.is_some() {
            // Empty registration confirms both HTTPS and enrolled identity.
            client.post(format!("{url}/v1/wrappers")).json(
                &underwrap_protocol::service::RuntimeWrapperRegisterRequest { wrappers: vec![] },
            )
        } else {
            client.get(format!("{url}/v1/admin/auth/bootstrap/status"))
        }
        .timeout((deadline - now).min(PROBE_TIMEOUT));
        match tokio::time::timeout_at(deadline, request.send()).await {
            Ok(Ok(response)) if response.status().is_success() => {
                backend_alive().context("backend exited during HTTPS readiness")?;
                return Ok(());
            }
            Ok(Ok(response)) => last = format!("HTTP {}", response.status()),
            Ok(Err(error)) => {
                last = if error.is_timeout() {
                    "request timeout"
                } else {
                    "transport error"
                }
                .into()
            }
            Err(_) => last = "elapsed deadline".into(),
        }
        backend_alive().context("backend exited during HTTPS readiness")?;
        tokio::time::sleep_until((tokio::time::Instant::now() + PROBE_INTERVAL).min(deadline))
            .await;
    }
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
