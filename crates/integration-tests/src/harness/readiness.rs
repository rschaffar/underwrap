//! Readiness probes: port picking and wait-for-URL/path helpers.

use anyhow::Result;
use std::path::Path;

pub(super) fn pick_unused_port() -> Result<u16> {
    use std::sync::atomic::{AtomicU16, Ordering};
    static NEXT_PORT: AtomicU16 = AtomicU16::new(43000);
    let port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);
    anyhow::ensure!(port < 60000, "ran out of test ports");
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
