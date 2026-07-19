//! Embedded test PKI material and filesystem layout helpers.

use anyhow::Result;
use std::path::{Path, PathBuf};

pub(crate) const TEST_CA_PEM: &str = include_str!("../../testdata/pki/ca.pem");
pub(crate) const TEST_SERVER_PEM: &str = include_str!("../../testdata/pki/server.pem");
pub(crate) const TEST_SERVER_KEY: &str = include_str!("../../testdata/pki/server.key");
pub(crate) const TEST_CLIENT_LOCAL_PEM: &str = include_str!("../../testdata/pki/client-local.pem");
pub(crate) const TEST_CLIENT_LOCAL_KEY: &str = include_str!("../../testdata/pki/client-local.key");
pub(crate) const TEST_CLIENT_TUNNEL_PEM: &str =
    include_str!("../../testdata/pki/client-tunnel.pem");
pub(crate) const TEST_CLIENT_TUNNEL_KEY: &str =
    include_str!("../../testdata/pki/client-tunnel.key");

pub(super) struct TestPkiPaths {
    pub ca_cert: PathBuf,
    pub server_cert: PathBuf,
    pub server_key: PathBuf,
    pub client_cert: PathBuf,
    pub client_key: PathBuf,
}

pub(super) fn principal_client_name(principal: &str) -> Result<&'static str> {
    match principal {
        "underwrap-local" => Ok("local"),
        "underwrap-tunnel" => Ok("tunnel"),
        other => anyhow::bail!("unknown test principal '{other}'"),
    }
}

pub(super) fn test_pki_paths(base: &Path, client_name: &str) -> Result<TestPkiPaths> {
    let pki_dir = base.join("pki");
    std::fs::create_dir_all(&pki_dir)?;
    // The daemon also requires the credential's parent directory to be
    // 0700 or tighter (matching the NixOS module's `${stateDir}` tmpfiles
    // entry). `create_dir_all` honors the umask (typically 0755), so pin it.
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&pki_dir, std::fs::Permissions::from_mode(0o700))?;
    }

    let ca_cert = pki_dir.join("ca.pem");
    let server_cert = pki_dir.join("server.pem");
    let server_key = pki_dir.join("server.key");
    std::fs::write(&ca_cert, TEST_CA_PEM.as_bytes())?;
    std::fs::write(&server_cert, TEST_SERVER_PEM.as_bytes())?;
    std::fs::write(&server_key, TEST_SERVER_KEY.as_bytes())?;

    let (client_cert, client_key, client_cert_bytes, client_key_bytes): (&str, &str, &[u8], &[u8]) =
        match client_name {
            "local" => (
                "client-local.pem",
                "client-local.key",
                TEST_CLIENT_LOCAL_PEM.as_bytes(),
                TEST_CLIENT_LOCAL_KEY.as_bytes(),
            ),
            "tunnel" => (
                "client-tunnel.pem",
                "client-tunnel.key",
                TEST_CLIENT_TUNNEL_PEM.as_bytes(),
                TEST_CLIENT_TUNNEL_KEY.as_bytes(),
            ),
            other => anyhow::bail!("unknown test client name '{other}'"),
        };
    let client_cert = pki_dir.join(client_cert);
    let client_key = pki_dir.join(client_key);
    std::fs::write(&client_cert, client_cert_bytes)?;
    std::fs::write(&client_key, client_key_bytes)?;
    // The daemon refuses to start unless its mTLS credential is 0600 or
    // tighter (see underwrapd startup::check_credential_permissions),
    // matching what underwrap-keygen writes. `std::fs::write` honors the
    // umask (typically 0644), so tighten explicitly here.
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&client_cert, std::fs::Permissions::from_mode(0o600))?;
        std::fs::set_permissions(&client_key, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(TestPkiPaths {
        ca_cert,
        server_cert,
        server_key,
        client_cert,
        client_key,
    })
}
