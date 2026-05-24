//! TLS plumbing for the backend HTTPS listener.
//!
//! [`build_https_acceptor`] wires the server-side certificate, private
//! key, and the fingerprint-based client-cert verifier into a
//! `TlsAcceptor`. The actual authorization step (mapping a client
//! cert to a runtime client instance) happens later in
//! `runtime_api::principal`; the TLS layer here just guarantees the
//! handshake signature checks out against the leaf's public key.

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

use crate::config::HttpsTlsConfig;

pub(crate) fn build_https_acceptor(config: &HttpsTlsConfig) -> Result<TlsAcceptor> {
    use rustls::ServerConfig;

    let _ = rustls::crypto::ring::default_provider().install_default();

    let certs = load_cert_chain(&config.cert_file)?;
    let key = load_private_key(&config.key_file)?;

    let verifier: Arc<dyn rustls::server::danger::ClientCertVerifier> =
        Arc::new(FingerprintClientCertVerifier::new());

    let server_config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(certs, key)
        .context("building rustls server config")?;

    Ok(TlsAcceptor::from(Arc::new(server_config)))
}

/// Client certificate verifier that accepts any presented leaf at the TLS
/// layer. Authorization is performed downstream by SHA-256 fingerprint lookup
/// against the runtime-client registry in `resolve_runtime_client`: the
/// chain, subject, EKU, and validity window of the leaf are not consulted
/// here, only its hash. An unknown-fingerprint cert still fails at the
/// application layer; a known-fingerprint cert is admitted regardless of how
/// it was signed.
///
/// The handshake signature is still verified against the leaf's public key so
/// a stolen cert without its private key cannot be replayed.
#[derive(Debug)]
struct FingerprintClientCertVerifier;

impl FingerprintClientCertVerifier {
    fn new() -> Self {
        Self
    }

    /// `install_default` and `default_provider` return the same
    /// `CryptoProvider` instance; reaching for it on every signature
    /// check is a cheap pointer follow and removes the cached copy.
    fn algs() -> rustls::crypto::WebPkiSupportedAlgorithms {
        rustls::crypto::ring::default_provider().signature_verification_algorithms
    }
}

impl rustls::server::danger::ClientCertVerifier for FingerprintClientCertVerifier {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        false
    }

    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::server::danger::ClientCertVerified, rustls::Error> {
        Ok(rustls::server::danger::ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &Self::algs())
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &Self::algs())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        Self::algs().supported_schemes()
    }
}

fn load_cert_chain(path: &Path) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    let pem = std::fs::read(path)
        .with_context(|| format!("reading certificate file {}", path.display()))?;
    let mut reader = std::io::BufReader::new(pem.as_slice());
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("parsing certificate PEM")?;
    anyhow::ensure!(
        !certs.is_empty(),
        "certificate file '{}' contained no certificates",
        path.display()
    );
    Ok(certs)
}

fn load_private_key(path: &Path) -> Result<rustls::pki_types::PrivateKeyDer<'static>> {
    let pem = std::fs::read(path)
        .with_context(|| format!("reading private key file {}", path.display()))?;
    let mut reader = std::io::BufReader::new(pem.as_slice());
    if let Some(key) =
        rustls_pemfile::private_key(&mut reader).context("parsing private key PEM")?
    {
        Ok(key)
    } else {
        anyhow::bail!(
            "private key file '{}' contained no private key",
            path.display()
        )
    }
}
