use anyhow::{Context, Result, bail};
use clap::Parser;
use std::io::{BufRead, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
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
    /// with `/v1/enroll`, the helper appends it.
    #[arg(long)]
    pub url: String,
    /// One-time invite id from the operator-approved enrollment flow.
    #[arg(long)]
    pub invite_id: String,
    /// CA bundle used to verify the backend service certificate.
    #[arg(long)]
    pub ca_file: PathBuf,
    /// Where to atomically install the issued client certificate PEM.
    #[arg(long)]
    pub client_cert_out: PathBuf,
    /// Where to atomically install the locally generated client private key PEM.
    #[arg(long)]
    pub client_key_out: PathBuf,
    /// Optional output path for the certificate issuer PEM returned by the backend.
    #[arg(long)]
    pub issuer_ca_out: Option<PathBuf>,
}

pub fn run(cli: Cli) -> Result<()> {
    harden_process()?;
    validate_output_paths(
        &cli.client_cert_out,
        &cli.client_key_out,
        cli.issuer_ca_out.as_deref(),
    )?;

    eprintln!("underwrap-enroll: generating local client key and CSR");
    let (key_pem, csr_pem) = generate_key_and_csr()?;

    let mut secret = prompt_enrollment_secret()?;
    let enrollment_url = normalize_enrollment_url(&cli.url)?;

    eprintln!("underwrap-enroll: submitting CSR to backend");
    let response = submit_enrollment(
        &enrollment_url,
        &cli.ca_file,
        EnrollmentSubmitRequest {
            invite_id: cli.invite_id,
            enrollment_secret: secret.to_string(),
            csr_pem,
        },
    )?;
    secret.zeroize();

    eprintln!("underwrap-enroll: installing credential files atomically");
    install_credential(
        &cli.client_key_out,
        key_pem.as_bytes(),
        0o600,
        &cli.client_cert_out,
        response.cert_pem.as_bytes(),
        0o644,
        cli.issuer_ca_out
            .as_deref()
            .map(|path| (path, response.ca_cert_pem.as_bytes(), 0o644)),
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

fn validate_output_paths(
    cert_out: &Path,
    key_out: &Path,
    issuer_ca_out: Option<&Path>,
) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for path in [Some(cert_out), Some(key_out), issuer_ca_out]
        .into_iter()
        .flatten()
    {
        let key = path.as_os_str().to_owned();
        anyhow::ensure!(seen.insert(key), "output paths must be distinct");
        anyhow::ensure!(
            !path.as_os_str().is_empty(),
            "output path must not be empty"
        );
    }
    Ok(())
}

fn generate_key_and_csr() -> Result<(Zeroizing<String>, String)> {
    let key_pair = rcgen::KeyPair::generate().context("generating client keypair")?;
    let key_pem = Zeroizing::new(key_pair.serialize_pem());

    let mut dn = rcgen::DistinguishedName::new();
    dn.push(rcgen::DnType::CommonName, "underwrap-enrollment");
    let mut params = rcgen::CertificateParams::default();
    params.distinguished_name = dn;
    let csr_pem = params
        .serialize_request(&key_pair)
        .context("generating CSR")?
        .pem()
        .context("encoding CSR as PEM")?;
    Ok((key_pem, csr_pem))
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
    ca_file: &Path,
    request: EnrollmentSubmitRequest,
) -> Result<EnrollmentSubmitResponse> {
    let ca_pem = std::fs::read(ca_file)
        .with_context(|| format!("reading backend CA bundle {}", ca_file.display()))?;
    let client = reqwest::blocking::Client::builder()
        .use_rustls_tls()
        .timeout(std::time::Duration::from_secs(15))
        .add_root_certificate(
            reqwest::Certificate::from_pem(&ca_pem).context("parsing backend CA bundle PEM")?,
        )
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

fn write_atomic(path: &Path, contents: &[u8], mode: u32) -> Result<()> {
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
    issuer_ca: Option<(&Path, &[u8], u32)>,
) -> Result<()> {
    write_atomic(key_out, key_pem, key_mode)?;
    write_atomic(cert_out, cert_pem, cert_mode)?;
    if let Some((path, pem, mode)) = issuer_ca {
        write_atomic(path, pem, mode)?;
    }
    Ok(())
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
        let err = validate_output_paths(path, path, None).unwrap_err();
        assert!(format!("{err:#}").contains("must be distinct"));
    }

    #[test]
    fn key_and_csr_are_generated_locally() {
        let (key_pem, csr_pem) = generate_key_and_csr().unwrap();
        assert!(key_pem.contains("BEGIN PRIVATE KEY"));
        assert!(csr_pem.contains("BEGIN CERTIFICATE REQUEST"));
    }

    #[cfg(unix)]
    #[test]
    fn install_credential_writes_expected_permissions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let key = tmp.path().join("client.key");
        let cert = tmp.path().join("client.pem");
        let ca = tmp.path().join("issuer.pem");
        install_credential(
            &key,
            b"KEY",
            0o600,
            &cert,
            b"CERT",
            0o644,
            Some((&ca, b"CA", 0o644)),
        )
        .unwrap();
        assert_eq!(std::fs::read(&key).unwrap(), b"KEY");
        assert_eq!(std::fs::read(&cert).unwrap(), b"CERT");
        assert_eq!(std::fs::read(&ca).unwrap(), b"CA");
        assert_eq!(
            std::fs::metadata(&key).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&cert).unwrap().permissions().mode() & 0o777,
            0o644
        );
        assert_eq!(
            std::fs::metadata(&ca).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }
}
