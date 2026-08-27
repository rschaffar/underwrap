use anyhow::{Context, Result, bail};
use clap::Parser;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use underwrap_protocol::config::{BackendServiceConfig, is_canonical_absolute_path};
use zeroize::Zeroizing;

#[derive(Debug, Parser, Clone)]
#[command(
    name = "underwrap-keygen",
    about = "Generate a self-signed client credential for underwrap"
)]
pub struct Cli {
    /// Where to atomically install the generated combined client identity PEM.
    /// Required unless `--instance-config` is given.
    #[arg(long, conflicts_with = "instance_config")]
    pub client_identity_out: Option<PathBuf>,
    /// Path to an underwrapd daemon config (typically
    /// `/etc/underwrap/daemon/<instance>.yaml` — the file that carries
    /// the `backend` section, not the `/etc/underwrap/instances/`
    /// underwrap-exec sidecar). When given, the helper reads
    /// `backend.client_identity_file` from that file and refuses a duplicate
    /// explicit `--client-identity-out`.
    #[arg(long)]
    pub instance_config: Option<PathBuf>,
}

/// Effective output location, resolved either from the explicit CLI flag or
/// from an instance config file.
#[derive(Debug, Clone)]
pub struct ResolvedTargets {
    pub client_identity_out: PathBuf,
    /// When the target came from `--instance-config` and an
    /// `underwrap-<catalog_instance>` user + group could be resolved on
    /// this host, the (uid, gid) the helper should chown the freshly written
    /// identity file to. `None` falls back to "keep current uid/gid"
    /// (root, under `sudo`) plus a warning so the operator knows to
    /// adjust ownership manually.
    pub credential_owner: Option<(u32, u32)>,
}

impl Cli {
    pub fn resolve_targets(&self) -> Result<ResolvedTargets> {
        if let Some(path) = self.instance_config.as_deref() {
            let yaml = std::fs::read_to_string(path)
                .with_context(|| format!("reading instance config '{}'", path.display()))?;
            // keygen only needs the backend credential paths, so it
            // pulls out just the `backend` section rather than the full
            // `DaemonConfig` (whose daemon-only fields are irrelevant
            // here and would otherwise force every keygen config to
            // carry them).
            let doc: serde_yaml::Value = serde_yaml::from_str(&yaml)
                .with_context(|| format!("parsing instance config '{}'", path.display()))?;
            let backend_value = doc.get("backend").cloned().with_context(|| {
                format!(
                    "instance config '{}' has no backend section",
                    path.display()
                )
            })?;
            let backend: BackendServiceConfig = serde_yaml::from_value(backend_value)
                .with_context(|| {
                    format!(
                        "parsing backend section of instance config '{}'",
                        path.display()
                    )
                })?;
            let client_identity_out = backend.client_identity_file.with_context(|| {
                format!(
                    "instance config '{}' has no backend.client_identity_file",
                    path.display()
                )
            })?;
            // The instance name is the config file's stem
            // (`/etc/underwrap/daemon/work.yaml` → `work`). The
            // NixOS module names the daemon user `underwrap-<instance>`
            // by the same convention, so this preserves chown without
            // needing a redundant field inside the config.
            let credential_owner = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(derive_instance_owner);
            Ok(ResolvedTargets {
                client_identity_out,
                credential_owner,
            })
        } else {
            let client_identity_out = self
                .client_identity_out
                .clone()
                .context("either --instance-config or --client-identity-out is required")?;
            Ok(ResolvedTargets {
                client_identity_out,
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
    validate_output_path(&targets.client_identity_out)?;

    eprintln!("underwrap-keygen: generating client key and self-signed cert");
    let (identity_pem, cert_pem) = generate_key_and_self_signed_cert()?;

    eprintln!("underwrap-keygen: installing client identity atomically");
    install_identity(
        &targets.client_identity_out,
        identity_pem.as_slice(),
        targets.credential_owner,
    )?;

    eprintln!(
        "underwrap-keygen: wrote {}",
        targets.client_identity_out.display()
    );
    eprintln!(
        "underwrap-keygen: paste the certificate PEM below into the admin UI to register this client"
    );

    // Persistence is already committed here. Stdout delivery is intentionally
    // nontransactional: a write failure leaves the complete new identity installed.
    let mut stdout = std::io::stdout().lock();
    write_public_certificate(&mut stdout, &cert_pem)?;
    Ok(())
}

fn write_public_certificate(writer: &mut impl Write, cert_pem: &str) -> Result<()> {
    writer
        .write_all(cert_pem.as_bytes())
        .and_then(|()| writer.flush())
        .context("writing public certificate to stdout after identity commit")
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

fn validate_output_path(path: &Path) -> Result<()> {
    anyhow::ensure!(
        is_canonical_absolute_path(path),
        "output path must be a canonical absolute path"
    );
    Ok(())
}

fn generate_key_and_self_signed_cert() -> Result<(Zeroizing<Vec<u8>>, String)> {
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
    let cert_pem = cert.pem();
    let mut identity_pem = Zeroizing::new(Vec::with_capacity(cert_pem.len() + key_pem.len()));
    identity_pem.extend_from_slice(cert_pem.as_bytes());
    identity_pem.extend_from_slice(key_pem.as_bytes());
    Ok((identity_pem, cert_pem))
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstallPhase {
    Write,
    Chown,
    Chmod,
    Sync,
    Persist,
}

#[cfg(test)]
thread_local! {
    static FAIL_INSTALL_PHASE: std::cell::Cell<Option<InstallPhase>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn install_checkpoint(phase: InstallPhase) -> Result<()> {
    if FAIL_INSTALL_PHASE.with(|selected| selected.get() == Some(phase)) {
        bail!("injected identity install failure at {phase:?}");
    }
    Ok(())
}

macro_rules! install_checkpoint {
    ($phase:ident) => {
        #[cfg(test)]
        install_checkpoint(InstallPhase::$phase)?;
    };
}

fn write_identity_atomic(path: &Path, contents: &[u8], owner: Option<(u32, u32)>) -> Result<()> {
    use std::os::fd::AsRawFd;
    let parent = path.parent().ok_or_else(|| {
        anyhow::anyhow!("output path '{}' has no parent directory", path.display())
    })?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating output directory {}", parent.display()))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("creating temp file under {}", parent.display()))?;

    install_checkpoint!(Write);
    temp.write_all(contents)
        .with_context(|| format!("writing temp identity for {}", path.display()))?;

    if let Some((uid, gid)) = owner {
        install_checkpoint!(Chown);
        let ret = unsafe { libc::fchown(temp.as_file().as_raw_fd(), uid, gid) };
        if ret != 0 {
            bail!(
                "fchown({}, {uid}, {gid}) failed: {}",
                path.display(),
                std::io::Error::last_os_error()
            );
        }
    }

    install_checkpoint!(Chmod);
    temp.as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o400))
        .with_context(|| {
            format!(
                "setting permissions on temp identity for {}",
                path.display()
            )
        })?;

    install_checkpoint!(Sync);
    temp.as_file()
        .sync_all()
        .with_context(|| format!("syncing prepared identity for {}", path.display()))?;

    // This persist is the single visibility commit. The prepared file is synced,
    // but the parent directory is intentionally not fsynced for power-loss durability.
    install_checkpoint!(Persist);
    temp.persist(path)
        .map_err(|e| anyhow::anyhow!("atomically persisting {}: {e}", path.display()))?;
    Ok(())
}

fn install_identity(
    identity_out: &Path,
    identity_pem: &[u8],
    preferred_owner: Option<(u32, u32)>,
) -> Result<()> {
    let owner = resolve_target_owner(identity_out, preferred_owner);
    if preferred_owner.is_none() && owner.is_none() {
        eprintln!(
            "underwrap-keygen: warning: could not infer a daemon user for the client identity; \
             writing as the current user. chown {} to the daemon user before starting underwrapd.",
            identity_out.display()
        );
    }
    write_identity_atomic(identity_out, identity_pem, owner)
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
