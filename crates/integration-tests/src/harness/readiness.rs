//! Readiness probes: port picking and wait-for-URL/path helpers.

use anyhow::{Context, Result};
use std::path::Path;

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
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .context("binding ephemeral port for harness")?;
    let port = listener
        .local_addr()
        .context("reading local_addr from ephemeral listener")?
        .port();
    drop(listener);
    Ok(port)
}

pub(super) async fn wait_for_admin(url: &str, token: &str) -> Result<()> {
    let client = reqwest::Client::new();
    for _ in 0..100 {
        if let Ok(response) = client
            .get(format!("{url}/v1/admin/stores"))
            .bearer_auth(token)
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

pub(super) async fn wait_for_service(
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
        .add_root_certificate(reqwest::Certificate::from_pem(&ca_pem)?)
        .identity(reqwest::Identity::from_pem(&identity_pem)?)
        .build()?;

    for _ in 0..100 {
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
