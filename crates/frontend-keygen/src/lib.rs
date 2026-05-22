use anyhow::{Context, Result, bail};
use clap::Parser;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use underwrap_protocol::config::DaemonConfig;
use zeroize::Zeroizing;

#[derive(Debug, Parser, Clone)]
#[command(
    name = "underwrap-keygen",
    about = "Generate a self-signed client credential for underwrap"
)]
pub struct Cli {
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
    /// helper reads `backend.client_cert_file` and
    /// `backend.client_key_file` from that file and refuses any
    /// duplicate explicit `--client-cert-out` / `--client-key-out`.
    #[arg(long)]
    pub instance_config: Option<PathBuf>,
}

/// Effective output locations, resolved either from explicit CLI flags or
/// from an instance config file.
#[derive(Debug, Clone)]
pub struct ResolvedTargets {
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
            // The instance name is the config file's stem
            // (`/etc/underwrap/instances/work.yaml` → `work`). The
            // NixOS module names the daemon user `underwrap-<instance>`
            // by the same convention, so this preserves chown without
            // needing a redundant field inside the config.
            let credential_owner = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(derive_instance_owner);
            Ok(ResolvedTargets {
                client_cert_out,
                client_key_out,
                credential_owner,
            })
        } else {
            let client_cert_out = self
                .client_cert_out
                .clone()
                .context("either --instance-config or --client-cert-out is required")?;
            let client_key_out = self
                .client_key_out
                .clone()
                .context("either --instance-config or --client-key-out is required")?;
            Ok(ResolvedTargets {
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
/// (e.g., generating credentials on a workstation that doesn't run the daemon).
fn derive_instance_owner(instance: &str) -> Option<(u32, u32)> {
    let inst = instance.trim();
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

pub fn run(cli: Cli) -> Result<()> {
    harden_process()?;
    let targets = cli.resolve_targets()?;
    validate_output_paths(&targets.client_cert_out, &targets.client_key_out)?;

    eprintln!("underwrap-keygen: generating client key and self-signed cert");
    let (key_pem, cert_pem) = generate_key_and_self_signed_cert()?;

    eprintln!("underwrap-keygen: installing credential files atomically");
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
        "underwrap-keygen: wrote {} and {}",
        targets.client_key_out.display(),
        targets.client_cert_out.display()
    );
    eprintln!(
        "underwrap-keygen: paste the certificate PEM below into the admin UI to register this client"
    );

    // The PEM is the only thing on stdout — easy to pipe or copy.
    print!("{cert_pem}");
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
            "underwrap-keygen: warning: could not infer a daemon user for the credential files; \
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
/// ownership for the re-key case; otherwise we leave it to the
/// process's current uid/gid.
fn resolve_target_owner(path: &Path, preferred: Option<(u32, u32)>) -> Option<(u32, u32)> {
    if preferred.is_some() {
        return preferred;
    }
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| (m.uid(), m.gid()))
}

#[cfg(test)]
mod tests;
