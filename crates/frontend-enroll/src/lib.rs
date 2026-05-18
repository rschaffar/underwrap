use anyhow::{Context, Result, bail};
use clap::Parser;
use sha2::{Digest, Sha256};
use std::io::{BufRead, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use underwrap_protocol::config::DaemonConfig;
use underwrap_protocol::service::{
    EnrollmentSubmitRequest, EnrollmentSubmitResponse, ServiceErrorResponse,
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug, Parser, Clone)]
#[command(
    name = "underwrap-enroll",
    about = "Enroll this underwrap instance with the backend"
)]
pub struct Cli {
    /// Enrollment endpoint URL or backend base URL. If the path does not end
    /// with `/v1/enroll`, the helper appends it. Required unless
    /// `--instance-config` is given.
    #[arg(long, conflicts_with = "instance_config")]
    pub url: Option<String>,
    /// One-time invite id from the operator-approved enrollment flow.
    #[arg(long)]
    pub invite_id: String,
    /// Where to atomically install the generated client certificate PEM.
    /// Required unless `--instance-config` is given.
    #[arg(long, conflicts_with = "instance_config")]
    pub client_cert_out: Option<PathBuf>,
    /// Where to atomically install the generated client private key PEM.
    /// Required unless `--instance-config` is given.
    #[arg(long, conflicts_with = "instance_config")]
    pub client_key_out: Option<PathBuf>,
    /// Path to an underwrapd instance config (typically
    /// `/etc/underwrap/instances/<instance>.yaml`). When given, the
    /// helper reads `backend.url`, `backend.client_cert_file`, and
    /// `backend.client_key_file` from that file and refuses any
    /// duplicate explicit `--url` / `--client-cert-out` /
    /// `--client-key-out`.
    #[arg(long)]
    pub instance_config: Option<PathBuf>,
    /// Pinned SHA-256 fingerprint (hex, lowercase, optional colons) of the
    /// backend's TLS server certificate. When set, the helper accepts that
    /// exact leaf without consulting any CA. Use this for backends served
    /// by a private CA or a self-signed cert. When unset, the backend's
    /// server cert is verified against the host's system trust store; if
    /// that verification fails the helper prints the presented certificate
    /// and its fingerprint so an operator can decide whether to pin it.
    #[arg(long)]
    pub accept_server_ca_fingerprint: Option<String>,
}

/// Effective output locations and backend URL, resolved either from
/// explicit CLI flags or from an instance config file.
#[derive(Debug, Clone)]
pub struct ResolvedTargets {
    pub url: String,
    pub client_cert_out: PathBuf,
    pub client_key_out: PathBuf,
    /// When the targets came from `--instance-config` and an
    /// `underwrap-<catalog_instance>` user + group could be resolved on
    /// this host, the (uid, gid) the helper should chown freshly written
    /// credential files to. `None` falls back to "keep current uid/gid"
    /// (root, under `sudo`) plus a warning so the operator knows to
    /// adjust ownership manually.
    pub credential_owner: Option<(u32, u32)>,
}

impl Cli {
    pub fn resolve_targets(&self) -> Result<ResolvedTargets> {
        if let Some(path) = self.instance_config.as_deref() {
            let yaml = std::fs::read_to_string(path)
                .with_context(|| format!("reading instance config '{}'", path.display()))?;
            let cfg: DaemonConfig = serde_yaml::from_str(&yaml)
                .with_context(|| format!("parsing instance config '{}'", path.display()))?;
            anyhow::ensure!(
                !cfg.backend.url.trim().is_empty(),
                "instance config '{}' has an empty backend.url",
                path.display()
            );
            let client_cert_out = cfg.backend.client_cert_file.with_context(|| {
                format!(
                    "instance config '{}' has no backend.client_cert_file",
                    path.display()
                )
            })?;
            let client_key_out = cfg.backend.client_key_file.with_context(|| {
                format!(
                    "instance config '{}' has no backend.client_key_file",
                    path.display()
                )
            })?;
            let credential_owner = derive_instance_owner(&cfg.catalog_instance);
            Ok(ResolvedTargets {
                url: cfg.backend.url,
                client_cert_out,
                client_key_out,
                credential_owner,
            })
        } else {
            let url = self
                .url
                .clone()
                .context("either --instance-config or --url is required")?;
            let client_cert_out = self
                .client_cert_out
                .clone()
                .context("either --instance-config or --client-cert-out is required")?;
            let client_key_out = self
                .client_key_out
                .clone()
                .context("either --instance-config or --client-key-out is required")?;
            Ok(ResolvedTargets {
                url,
                client_cert_out,
                client_key_out,
                credential_owner: None,
            })
        }
    }
}

/// Derive the daemon `(uid, gid)` for an instance using the NixOS module's
/// `underwrap-<instance>` naming convention. Returns `None` if either the
/// instance name is empty or the user/group doesn't exist on this host
/// (e.g., enrolling on a workstation that doesn't run the daemon).
fn derive_instance_owner(catalog_instance: &str) -> Option<(u32, u32)> {
    let inst = catalog_instance.trim();
    if inst.is_empty() {
        return None;
    }
    let name = format!("underwrap-{inst}");
    let uid = lookup_uid(&name)?;
    let gid = lookup_gid(&name)?;
    Some((uid, gid))
}

fn lookup_uid(name: &str) -> Option<u32> {
    let cname = std::ffi::CString::new(name).ok()?;
    // getpwnam is not reentrant but the helper is single-threaded.
    let pw = unsafe { libc::getpwnam(cname.as_ptr()) };
    if pw.is_null() {
        None
    } else {
        Some(unsafe { (*pw).pw_uid })
    }
}

fn lookup_gid(name: &str) -> Option<u32> {
    let cname = std::ffi::CString::new(name).ok()?;
    let gr = unsafe { libc::getgrnam(cname.as_ptr()) };
    if gr.is_null() {
        None
    } else {
        Some(unsafe { (*gr).gr_gid })
    }
}

/// How the helper should authenticate the backend's TLS server cert.
#[derive(Debug, Clone)]
pub enum ServerCertTrust {
    /// Verify against the host's native trust store (the usual flow).
    SystemRoots,
    /// Accept only the leaf whose SHA-256 fingerprint matches this hex
    /// string (lowercase, no colons).
    PinnedFingerprint(String),
}

pub fn run(cli: Cli) -> Result<()> {
    let secret = prompt_enrollment_secret()?;
    run_with_secret(cli, secret)
}

pub fn run_with_secret(cli: Cli, mut secret: Zeroizing<String>) -> Result<()> {
    harden_process()?;
    let targets = cli.resolve_targets()?;
    validate_output_paths(&targets.client_cert_out, &targets.client_key_out)?;

    let trust = match cli.accept_server_ca_fingerprint.as_deref() {
        Some(raw) => ServerCertTrust::PinnedFingerprint(normalize_fingerprint(raw)?),
        None => ServerCertTrust::SystemRoots,
    };

    eprintln!("underwrap-enroll: generating local client key and self-signed cert");
    let (key_pem, cert_pem) = generate_key_and_self_signed_cert()?;

    let enrollment_url = normalize_enrollment_url(&targets.url)?;

    eprintln!("underwrap-enroll: submitting cert to backend");
    let response = match submit_enrollment(
        &enrollment_url,
        &trust,
        EnrollmentSubmitRequest {
            invite_id: cli.invite_id.clone(),
            enrollment_secret: secret.to_string(),
            cert_pem: cert_pem.clone(),
        },
    ) {
        Ok(resp) => resp,
        Err(err) => {
            if matches!(trust, ServerCertTrust::SystemRoots) && looks_like_tls_error(&err) {
                report_unverified_server_certificate(&enrollment_url)?;
            }
            return Err(err);
        }
    };
    secret.zeroize();

    eprintln!("underwrap-enroll: installing credential files atomically");
    install_credential(
        &targets.client_key_out,
        key_pem.as_bytes(),
        0o400,
        &targets.client_cert_out,
        cert_pem.as_bytes(),
        0o440,
        targets.credential_owner,
    )?;

    eprintln!(
        "underwrap-enroll: enrollment complete (client_instance_id={}, cert_fingerprint={})",
        response.client_instance_id, response.cert_fingerprint
    );
    Ok(())
}

fn harden_process() -> Result<()> {
    let ret = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) };
    if ret != 0 {
        bail!(
            "prctl(PR_SET_DUMPABLE, 0) failed: {}",
            std::io::Error::last_os_error()
        );
    }
    let rlim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    let ret = unsafe { libc::setrlimit(libc::RLIMIT_CORE, &rlim) };
    if ret != 0 {
        bail!(
            "setrlimit(RLIMIT_CORE, 0) failed: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

pub fn normalize_enrollment_url(input: &str) -> Result<String> {
    let trimmed = input.trim().trim_end_matches('/');
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        bail!("--url must be an absolute http:// or https:// URL");
    }
    if trimmed.ends_with("/v1/enroll") {
        Ok(trimmed.to_string())
    } else {
        Ok(format!("{trimmed}/v1/enroll"))
    }
}

fn validate_output_paths(cert_out: &Path, key_out: &Path) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for path in [cert_out, key_out] {
        let key = path.as_os_str().to_owned();
        anyhow::ensure!(seen.insert(key), "output paths must be distinct");
        anyhow::ensure!(
            !path.as_os_str().is_empty(),
            "output path must not be empty"
        );
    }
    Ok(())
}

fn generate_key_and_self_signed_cert() -> Result<(Zeroizing<String>, String)> {
    let key_pair = rcgen::KeyPair::generate().context("generating client keypair")?;
    let key_pem = Zeroizing::new(key_pair.serialize_pem());

    // The backend authenticates this cert by SHA-256 fingerprint of the leaf;
    // it never consults the subject, SAN, EKU, or validity window. We still
    // set sensible client-auth-shaped values so standard tooling (openssl,
    // wire-level inspectors) renders the cert without surprises.
    let mut dn = rcgen::DistinguishedName::new();
    dn.push(rcgen::DnType::CommonName, "underwrap-client");
    let mut params = rcgen::CertificateParams::default();
    params.distinguished_name = dn;
    params.is_ca = rcgen::IsCa::NoCa;
    params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth];
    let cert = params
        .self_signed(&key_pair)
        .context("generating self-signed client cert")?;
    Ok((key_pem, cert.pem()))
}

struct EchoDisabledGuard {
    tty: std::fs::File,
    original: libc::termios,
}

impl EchoDisabledGuard {
    fn new() -> Result<Self> {
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("opening /dev/tty for interactive secret prompt")?;
        let fd = std::os::fd::AsRawFd::as_raw_fd(&tty);
        let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
        let ret = unsafe { libc::tcgetattr(fd, &mut original) };
        if ret != 0 {
            bail!(
                "tcgetattr(/dev/tty) failed: {}",
                std::io::Error::last_os_error()
            );
        }
        let mut updated = original;
        updated.c_lflag &= !libc::ECHO;
        let ret = unsafe { libc::tcsetattr(fd, libc::TCSANOW, &updated) };
        if ret != 0 {
            bail!(
                "tcsetattr(/dev/tty) failed: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(Self { tty, original })
    }
}

impl Drop for EchoDisabledGuard {
    fn drop(&mut self) {
        let fd = std::os::fd::AsRawFd::as_raw_fd(&self.tty);
        let _ = unsafe { libc::tcsetattr(fd, libc::TCSANOW, &self.original) };
    }
}

fn prompt_enrollment_secret() -> Result<Zeroizing<String>> {
    let mut guard = EchoDisabledGuard::new()?;
    write!(guard.tty, "Enrollment secret: ").context("writing prompt to /dev/tty")?;
    guard.tty.flush().context("flushing secret prompt")?;

    let mut reader = std::io::BufReader::new(&guard.tty);
    let mut secret = String::new();
    reader
        .read_line(&mut secret)
        .context("reading enrollment secret from /dev/tty")?;
    writeln!(guard.tty).ok();

    let trimmed = secret.trim_end_matches(['\r', '\n']).to_string();
    let secret = Zeroizing::new(trimmed);
    anyhow::ensure!(!secret.is_empty(), "enrollment secret must not be empty");
    Ok(secret)
}

fn submit_enrollment(
    url: &str,
    trust: &ServerCertTrust,
    request: EnrollmentSubmitRequest,
) -> Result<EnrollmentSubmitResponse> {
    let tls_config = build_rustls_client_config(trust)?;
    let client = reqwest::blocking::Client::builder()
        .use_preconfigured_tls(tls_config)
        .timeout(Duration::from_secs(15))
        .build()
        .context("building HTTPS enrollment client")?;
    let response = client
        .post(url)
        .json(&request)
        .send()
        .with_context(|| format!("POST {url}"))?;

    if response.status().is_success() {
        return response
            .json()
            .context("parsing successful enrollment response JSON");
    }

    let status = response.status();
    let body = response
        .bytes()
        .context("reading enrollment error response body")?;
    if let Ok(error) = serde_json::from_slice::<ServiceErrorResponse>(&body) {
        bail!(
            "enrollment failed with HTTP {status}: {} ({})",
            error.message,
            error.code
        );
    }
    bail!(
        "enrollment failed with HTTP {status}: {}",
        String::from_utf8_lossy(&body)
    )
}

fn build_rustls_client_config(trust: &ServerCertTrust) -> Result<rustls::ClientConfig> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let provider = rustls::crypto::ring::default_provider();
    let supported_algs = provider.signature_verification_algorithms;
    let builder = rustls::ClientConfig::builder();
    let config = match trust {
        ServerCertTrust::SystemRoots => {
            let roots = load_system_roots()?;
            builder.with_root_certificates(roots).with_no_client_auth()
        }
        ServerCertTrust::PinnedFingerprint(fp) => {
            let verifier: Arc<dyn rustls::client::danger::ServerCertVerifier> = Arc::new(
                FingerprintServerCertVerifier::new(fp.clone(), supported_algs),
            );
            builder
                .dangerous()
                .with_custom_certificate_verifier(verifier)
                .with_no_client_auth()
        }
    };
    Ok(config)
}

fn load_system_roots() -> Result<rustls::RootCertStore> {
    let mut roots = rustls::RootCertStore::empty();
    let result = rustls_native_certs::load_native_certs();
    if !result.errors.is_empty() {
        for err in &result.errors {
            eprintln!("underwrap-enroll: warning loading a system root: {err}");
        }
    }
    anyhow::ensure!(
        !result.certs.is_empty(),
        "no system root certificates found — pass --accept-server-ca-fingerprint to pin the backend's cert instead"
    );
    for cert in result.certs {
        // Skip individual roots that rustls cannot parse; this matches
        // browser behaviour and prevents one bad anchor from breaking
        // an otherwise valid store.
        let _ = roots.add(cert);
    }
    Ok(roots)
}

/// Connect to the backend with an accept-any verifier just long enough to
/// extract the leaf certificate, print it for human inspection, and explain
/// how to pin it on the next run. The probe never submits the enrollment.
fn report_unverified_server_certificate(enrollment_url: &str) -> Result<()> {
    let (cert_der, fingerprint) = match probe_server_certificate(enrollment_url) {
        Ok(v) => v,
        Err(probe_err) => {
            eprintln!(
                "underwrap-enroll: server certificate could not be verified against the system trust store, \
                 and probing the server to print its certificate failed: {probe_err:#}"
            );
            return Ok(());
        }
    };
    let pem = cert_der_to_pem(&cert_der);
    eprintln!();
    eprintln!(
        "underwrap-enroll: the backend's TLS certificate is not trusted by the host's system roots."
    );
    eprintln!("Presented server certificate (PEM):");
    eprintln!("{pem}");
    eprintln!("SHA-256 fingerprint: {fingerprint}");
    eprintln!();
    eprintln!("If this fingerprint matches the backend's published value, re-run with:");
    eprintln!("    --accept-server-ca-fingerprint {fingerprint}");
    Ok(())
}

fn probe_server_certificate(enrollment_url: &str) -> Result<(Vec<u8>, String)> {
    let parsed = reqwest::Url::parse(enrollment_url)
        .with_context(|| format!("parsing enrollment URL '{enrollment_url}'"))?;
    let host = parsed
        .host_str()
        .with_context(|| format!("enrollment URL has no host: {enrollment_url}"))?;
    let port = parsed
        .port_or_known_default()
        .with_context(|| format!("enrollment URL has no port: {enrollment_url}"))?;

    let _ = rustls::crypto::ring::default_provider().install_default();
    let provider = rustls::crypto::ring::default_provider();
    let verifier: Arc<dyn rustls::client::danger::ServerCertVerifier> = Arc::new(
        AcceptAnyServerCertVerifier::new(provider.signature_verification_algorithms),
    );
    let config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();

    let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
        .with_context(|| format!("invalid TLS server name '{host}'"))?;
    let mut conn = rustls::ClientConnection::new(Arc::new(config), server_name)
        .context("starting rustls probe client connection")?;
    let mut sock = TcpStream::connect((host, port))
        .with_context(|| format!("connecting TCP to {host}:{port}"))?;
    sock.set_read_timeout(Some(Duration::from_secs(15)))?;
    sock.set_write_timeout(Some(Duration::from_secs(15)))?;
    // Drive the handshake until rustls has consumed the server hello and
    // populated the peer certificate chain.
    while conn.is_handshaking() {
        conn.complete_io(&mut sock)
            .context("completing TLS probe handshake")?;
    }
    let cert = conn
        .peer_certificates()
        .and_then(|certs| certs.first())
        .context("server presented no certificate")?
        .to_vec();
    let fingerprint = sha256_hex(&cert);
    Ok((cert, fingerprint))
}

fn cert_der_to_pem(der: &[u8]) -> String {
    let encoded = base64_encode(der);
    let mut out = String::with_capacity(encoded.len() + 64);
    out.push_str("-----BEGIN CERTIFICATE-----\n");
    for chunk in encoded.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).unwrap());
        out.push('\n');
    }
    out.push_str("-----END CERTIFICATE-----\n");
    out
}

fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        out.push(ALPHABET[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
}

fn sha256_hex(input: &[u8]) -> String {
    let digest = Sha256::digest(input);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn normalize_fingerprint(raw: &str) -> Result<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':')
        .collect::<String>()
        .to_ascii_lowercase();
    anyhow::ensure!(
        cleaned.len() == 64 && cleaned.chars().all(|c| c.is_ascii_hexdigit()),
        "--accept-server-ca-fingerprint must be 64 hex characters (SHA-256 of the server cert DER)"
    );
    Ok(cleaned)
}

fn looks_like_tls_error(err: &anyhow::Error) -> bool {
    let text = format!("{err:#}").to_ascii_lowercase();
    text.contains("invalid certificate")
        || text.contains("invalid peer certificate")
        || text.contains("unknownissuer")
        || text.contains("unknown issuer")
        || text.contains("certificate verify failed")
        || text.contains("bad certificate")
        || text.contains("tls handshake")
        || text.contains("handshake failure")
        || text.contains("certificate signature")
}

#[derive(Debug)]
struct FingerprintServerCertVerifier {
    expected_fingerprint: String,
    supported_algs: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl FingerprintServerCertVerifier {
    fn new(
        expected_fingerprint: String,
        supported_algs: rustls::crypto::WebPkiSupportedAlgorithms,
    ) -> Self {
        Self {
            expected_fingerprint,
            supported_algs,
        }
    }
}

impl rustls::client::danger::ServerCertVerifier for FingerprintServerCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let actual = sha256_hex(end_entity.as_ref());
        if actual == self.expected_fingerprint {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(format!(
                "server certificate fingerprint mismatch: expected {}, got {actual}",
                self.expected_fingerprint
            )))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.supported_algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.supported_algs)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.supported_algs.supported_schemes()
    }
}

#[derive(Debug)]
struct AcceptAnyServerCertVerifier {
    supported_algs: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl AcceptAnyServerCertVerifier {
    fn new(supported_algs: rustls::crypto::WebPkiSupportedAlgorithms) -> Self {
        Self { supported_algs }
    }
}

impl rustls::client::danger::ServerCertVerifier for AcceptAnyServerCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.supported_algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.supported_algs)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.supported_algs.supported_schemes()
    }
}

fn write_atomic(path: &Path, contents: &[u8], mode: u32, owner: Option<(u32, u32)>) -> Result<()> {
    use std::os::fd::AsRawFd;
    let parent = path.parent().ok_or_else(|| {
        anyhow::anyhow!("output path '{}' has no parent directory", path.display())
    })?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating output directory {}", parent.display()))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("creating temp file under {}", parent.display()))?;
    temp.write_all(contents)
        .with_context(|| format!("writing temp file for {}", path.display()))?;
    temp.as_file()
        .sync_all()
        .with_context(|| format!("fsyncing temp file for {}", path.display()))?;
    if let Some((uid, gid)) = owner {
        let fd = temp.as_file().as_raw_fd();
        let ret = unsafe { libc::fchown(fd, uid, gid) };
        if ret != 0 {
            bail!(
                "fchown({}, {uid}, {gid}) failed: {}",
                path.display(),
                std::io::Error::last_os_error()
            );
        }
    }
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("setting permissions on temp file for {}", path.display()))?;
    temp.persist(path)
        .map_err(|e| anyhow::anyhow!("atomically persisting {}: {e}", path.display(),))?;
    Ok(())
}

fn install_credential(
    key_out: &Path,
    key_pem: &[u8],
    key_mode: u32,
    cert_out: &Path,
    cert_pem: &[u8],
    cert_mode: u32,
    preferred_owner: Option<(u32, u32)>,
) -> Result<()> {
    let key_owner = resolve_target_owner(key_out, preferred_owner);
    let cert_owner = resolve_target_owner(cert_out, preferred_owner);
    if preferred_owner.is_none() && key_owner.is_none() {
        eprintln!(
            "underwrap-enroll: warning: could not infer a daemon user for the credential files; \
             writing as the current user. chown {} and {} to the daemon user before starting underwrapd.",
            key_out.display(),
            cert_out.display()
        );
    }
    write_atomic(key_out, key_pem, key_mode, key_owner)?;
    write_atomic(cert_out, cert_pem, cert_mode, cert_owner)?;
    Ok(())
}

/// Decide the (uid, gid) for a credential file. `preferred_owner` (from the
/// instance config) wins; otherwise we preserve the existing file's
/// ownership for the re-enrollment case; otherwise we leave it to the
/// process's current uid/gid.
fn resolve_target_owner(path: &Path, preferred: Option<(u32, u32)>) -> Option<(u32, u32)> {
    if preferred.is_some() {
        return preferred;
    }
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| (m.uid(), m.gid()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_enrollment_url_accepts_full_endpoint() {
        assert_eq!(
            normalize_enrollment_url("https://backend.example/v1/enroll").unwrap(),
            "https://backend.example/v1/enroll"
        );
    }

    #[test]
    fn normalize_enrollment_url_appends_endpoint() {
        assert_eq!(
            normalize_enrollment_url("https://backend.example/").unwrap(),
            "https://backend.example/v1/enroll"
        );
    }

    #[test]
    fn distinct_output_paths_are_required() {
        let path = Path::new("/tmp/same.pem");
        let err = validate_output_paths(path, path).unwrap_err();
        assert!(format!("{err:#}").contains("must be distinct"));
    }

    #[test]
    fn key_and_self_signed_cert_are_generated_locally() {
        let (key_pem, cert_pem) = generate_key_and_self_signed_cert().unwrap();
        assert!(key_pem.contains("BEGIN PRIVATE KEY"));
        assert!(cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(!cert_pem.contains("CERTIFICATE REQUEST"));
    }

    fn explicit_cli(url: &str, cert: &str, key: &str) -> Cli {
        Cli {
            url: Some(url.into()),
            invite_id: "enr_test".into(),
            client_cert_out: Some(PathBuf::from(cert)),
            client_key_out: Some(PathBuf::from(key)),
            instance_config: None,
            accept_server_ca_fingerprint: None,
        }
    }

    #[test]
    fn resolve_targets_uses_explicit_flags() {
        let cli = explicit_cli("https://x/", "/tmp/c.pem", "/tmp/c.key");
        let t = cli.resolve_targets().unwrap();
        assert_eq!(t.url, "https://x/");
        assert_eq!(t.client_cert_out, PathBuf::from("/tmp/c.pem"));
        assert_eq!(t.client_key_out, PathBuf::from("/tmp/c.key"));
    }

    #[test]
    fn resolve_targets_reads_instance_config() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp.path().join("work.yaml");
        std::fs::write(
            &cfg,
            r#"
backend:
  url: https://backend.example.internal:8443
  client_cert_file: /etc/underwrap/client-work.pem
  client_key_file: /etc/underwrap/client-work.key
catalog_host: deepthought
catalog_instance: work
allowed_users: []
wrappers: {}
"#,
        )
        .unwrap();
        let cli = Cli {
            url: None,
            invite_id: "enr_x".into(),
            client_cert_out: None,
            client_key_out: None,
            instance_config: Some(cfg),
            accept_server_ca_fingerprint: None,
        };
        let t = cli.resolve_targets().unwrap();
        assert_eq!(t.url, "https://backend.example.internal:8443");
        assert_eq!(
            t.client_cert_out,
            PathBuf::from("/etc/underwrap/client-work.pem")
        );
        assert_eq!(
            t.client_key_out,
            PathBuf::from("/etc/underwrap/client-work.key")
        );
    }

    #[test]
    fn resolve_targets_rejects_instance_config_without_cert_paths() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp.path().join("noop.yaml");
        std::fs::write(
            &cfg,
            r#"
backend:
  url: https://backend.example.internal:8443
catalog_host: deepthought
catalog_instance: work
allowed_users: []
wrappers: {}
"#,
        )
        .unwrap();
        let cli = Cli {
            url: None,
            invite_id: "enr_x".into(),
            client_cert_out: None,
            client_key_out: None,
            instance_config: Some(cfg),
            accept_server_ca_fingerprint: None,
        };
        let err = cli.resolve_targets().unwrap_err();
        assert!(format!("{err:#}").contains("client_cert_file"));
    }

    #[test]
    fn resolve_targets_requires_url_without_instance_config() {
        let cli = Cli {
            url: None,
            invite_id: "enr_x".into(),
            client_cert_out: Some(PathBuf::from("/tmp/c.pem")),
            client_key_out: Some(PathBuf::from("/tmp/c.key")),
            instance_config: None,
            accept_server_ca_fingerprint: None,
        };
        let err = cli.resolve_targets().unwrap_err();
        assert!(format!("{err:#}").contains("--url"));
    }

    #[test]
    fn normalize_fingerprint_accepts_clean_hex() {
        let fp = normalize_fingerprint(
            "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:aa:bb:cc:dd:ee:ff:00:11:22:33:44:55:66:77:88:99",
        )
        .unwrap();
        assert_eq!(fp.len(), 64);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!fp.contains(':'));
    }

    #[test]
    fn normalize_fingerprint_rejects_short_input() {
        assert!(normalize_fingerprint("deadbeef").is_err());
    }

    #[test]
    fn cert_der_to_pem_round_trip_yields_a_valid_block() {
        let pem = cert_der_to_pem(&[0u8, 1, 2, 3, 4, 5]);
        assert!(pem.starts_with("-----BEGIN CERTIFICATE-----\n"));
        assert!(pem.trim_end().ends_with("-----END CERTIFICATE-----"));
    }

    #[cfg(unix)]
    #[test]
    fn install_credential_writes_expected_permissions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let key = tmp.path().join("client.key");
        let cert = tmp.path().join("client.pem");
        install_credential(&key, b"KEY", 0o400, &cert, b"CERT", 0o440, None).unwrap();
        assert_eq!(std::fs::read(&key).unwrap(), b"KEY");
        assert_eq!(std::fs::read(&cert).unwrap(), b"CERT");
        assert_eq!(
            std::fs::metadata(&key).unwrap().permissions().mode() & 0o777,
            0o400
        );
        assert_eq!(
            std::fs::metadata(&cert).unwrap().permissions().mode() & 0o777,
            0o440
        );
    }

    #[test]
    fn derive_instance_owner_returns_none_for_unknown_user() {
        assert!(derive_instance_owner("definitely-not-an-existing-instance-xyzzy").is_none());
    }

    #[test]
    fn derive_instance_owner_returns_none_for_empty_instance() {
        assert!(derive_instance_owner("").is_none());
        assert!(derive_instance_owner("   ").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_target_owner_prefers_explicit_then_existing_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("client.pem");

        // No file, no preference → None
        assert!(resolve_target_owner(&path, None).is_none());

        // File exists → preserves its uid/gid
        std::fs::write(&path, b"x").unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        use std::os::unix::fs::MetadataExt;
        let (uid, gid) = (meta.uid(), meta.gid());
        assert_eq!(resolve_target_owner(&path, None), Some((uid, gid)));

        // Explicit preference wins over existing file
        assert_eq!(
            resolve_target_owner(&path, Some((12345, 67890))),
            Some((12345, 67890))
        );
    }
}
