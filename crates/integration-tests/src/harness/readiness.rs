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

pub(crate) fn csrf_cookie_header(session_cookie: &str, csrf_token: &str) -> String {
    format!("__Host-uw-session={session_cookie}; __Host-uw-csrf={csrf_token}")
}

pub(crate) async fn bootstrap_admin(
    url: &str,
    ca_cert: &Path,
    username: &str,
    password: &str,
) -> Result<(String, String)> {
    let client = admin_client(ca_cert, std::time::Duration::from_secs(5))?;
    let response = client
        .post(format!("{url}/v1/admin/auth/bootstrap"))
        .json(&serde_json::json!({
            "username": username,
            "password": password,
        }))
        .send()
        .await?;
    anyhow::ensure!(
        response.status().is_success(),
        "bootstrap returned {}",
        response.status()
    );
    let cookies = response.headers().get_all(reqwest::header::SET_COOKIE);
    let mut session_cookie = None;
    let mut csrf_cookie = None;
    for value in cookies.iter().filter_map(|v| v.to_str().ok()) {
        if session_cookie.is_none() {
            session_cookie = parse_cookie_value(value, "__Host-uw-session");
        }
        if csrf_cookie.is_none() {
            csrf_cookie = parse_cookie_value(value, "__Host-uw-csrf");
        }
    }
    let session_cookie = session_cookie.context("bootstrap response missing session cookie")?;
    let csrf_cookie = csrf_cookie.context("bootstrap response missing csrf cookie")?;
    Ok((session_cookie, csrf_cookie))
}

/// Ask the kernel for an unused TCP port on loopback, then release
/// it so the caller can bind it from the test fixture.
///
/// This pattern has a tiny race window — between `drop(listener)` and
/// the next bind, the kernel could reassign the port to a different
/// process. In practice, port assignment is roughly LIFO inside the
/// ephemeral range, and the next bind happens within microseconds, so
/// collisions are rare in the test harness. The previous monotonic
/// counter starting at 43000 had the much worse failure mode of
/// reusing the same port across `cargo test` runs while it was still
/// in `TIME_WAIT` (ticket 280). Holding the listener open until the
/// real server binds would close the race but requires restructuring
/// the harness — see the ticket for the deferred follow-up.
pub(crate) fn pick_unused_port() -> Result<u16> {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").context("binding ephemeral port for harness")?;
    let port = listener
        .local_addr()
        .context("reading local_addr from ephemeral listener")?
        .port();
    drop(listener);
    Ok(port)
}

// Total window: 3000 × 20 ms ≈ 60 s. The previous 2 s budget flaked
// under contended Nix-sandbox loads (ticket 281) — the loop exits as
// soon as the listener answers, so a longer ceiling costs nothing on
// the happy path. The contention is real: `cargo test --workspace`
// runs many test binaries in parallel, and backend startup can still
// be delayed substantially by general CPU / I/O contention.
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
    client_cert: &Path,
    client_key: &Path,
) -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ca_pem = std::fs::read(ca_cert)?;
    let cert_pem = std::fs::read(client_cert)?;
    let key_pem = std::fs::read(client_key)?;
    let mut identity_pem = cert_pem;
    identity_pem.extend_from_slice(&key_pem);
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(std::time::Duration::from_millis(500))
        .add_root_certificate(reqwest::Certificate::from_pem(&ca_pem)?)
        .identity(reqwest::Identity::from_pem(&identity_pem)?)
        .build()?;

    for _ in 0..3000 {
        if let Ok(response) = client.get(format!("{url}/v1/status")).send().await
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
