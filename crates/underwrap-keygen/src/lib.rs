use anyhow::{Context, Result, bail};
use clap::Parser;
use std::ffi::{CStr, CString};
use std::io::Write;
use std::mem::MaybeUninit;
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
        resolve_targets_with(self, &mut SystemAccountResolver)
    }
}

/// Derive the daemon `(uid, gid)` for an instance using the NixOS module's
/// `underwrap-<instance>` naming convention. Returns `None` if either the
/// instance name is empty or the user/group doesn't exist on this host
/// (e.g., generating credentials on a workstation that doesn't run the daemon).
/// Operational NSS failures are returned rather than treated as not-found.
#[cfg(test)]
fn derive_instance_owner(instance: &str) -> Result<Option<(u32, u32)>> {
    derive_instance_owner_with(instance, &mut SystemAccountResolver)
}

const NSS_BUFFER_FALLBACK: usize = 16 * 1024;
const NSS_BUFFER_MAX: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NssBufferHint {
    Unavailable,
    Value(i128),
}

#[derive(Clone, Copy, Debug)]
struct NssBufferLimits {
    fallback: usize,
    max: usize,
}

const NSS_BUFFER_LIMITS: NssBufferLimits = NssBufferLimits {
    fallback: NSS_BUFFER_FALLBACK,
    max: NSS_BUFFER_MAX,
};

fn sysconf_buffer_hint(key: libc::c_int) -> NssBufferHint {
    // SAFETY: `key` is one of the supported NSS buffer-size constants, and
    // sysconf returns a scalar hint without writing through any pointer.
    let value = unsafe { libc::sysconf(key) };
    if value == -1 {
        NssBufferHint::Unavailable
    } else {
        NssBufferHint::Value(i128::from(value))
    }
}

fn initial_nss_buffer_size(
    account_kind: &str,
    hint: NssBufferHint,
    limits: NssBufferLimits,
) -> Result<usize> {
    let hinted = match hint {
        NssBufferHint::Unavailable | NssBufferHint::Value(..=0) => limits.fallback,
        NssBufferHint::Value(value) => usize::try_from(value).with_context(|| {
            format!("{account_kind} NSS buffer-size hint {value} is not representable")
        })?,
    };
    anyhow::ensure!(
        hinted <= limits.max,
        "{account_kind} NSS buffer-size hint {hinted} exceeds the {}-byte maximum",
        limits.max
    );
    Ok(hinted)
}

fn reserve_nss_buffer(buffer: &mut Vec<MaybeUninit<u8>>, additional: usize) -> Result<()> {
    buffer
        .try_reserve_exact(additional)
        .context("allocating caller-owned NSS lookup buffer")
}

fn resize_nss_buffer(
    buffer: &mut Vec<MaybeUninit<u8>>,
    target_len: usize,
    reserve: &mut impl FnMut(&mut Vec<MaybeUninit<u8>>, usize) -> Result<()>,
) -> Result<()> {
    let additional = target_len
        .checked_sub(buffer.len())
        .context("NSS buffer target length is smaller than its current length")?;
    reserve(buffer, additional)?;
    buffer.resize_with(target_len, MaybeUninit::uninit);
    Ok(())
}

fn next_nss_buffer_size(current: usize, max: usize) -> Result<usize> {
    anyhow::ensure!(
        current < max,
        "NSS lookup exhausted the {max}-byte buffer maximum"
    );
    let doubled = current
        .checked_mul(2)
        .context("NSS buffer growth arithmetic overflow")?;
    Ok(doubled.min(max))
}

fn lookup_account<T>(
    name: &str,
    account_kind: &str,
    hint: NssBufferHint,
    limits: NssBufferLimits,
    mut reserve: impl FnMut(&mut Vec<MaybeUninit<u8>>, usize) -> Result<()>,
    mut lookup: impl FnMut(&CStr, *mut T, *mut libc::c_char, usize, *mut *mut T) -> libc::c_int,
    id: impl FnOnce(&T) -> u32,
) -> Result<Option<u32>> {
    let cname = CString::new(name)
        .with_context(|| format!("{account_kind} NSS lookup name contains an interior NUL"))?;
    let initial = initial_nss_buffer_size(account_kind, hint, limits)?;
    let mut buffer = Vec::new();
    resize_nss_buffer(&mut buffer, initial, &mut reserve)?;
    let mut output = MaybeUninit::<T>::uninit();

    loop {
        let mut result = std::ptr::null_mut();
        let status = lookup(
            &cname,
            output.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        );
        if status == libc::ERANGE {
            let next = next_nss_buffer_size(buffer.len(), limits.max).with_context(|| {
                format!("resolving {account_kind} account {name:?} through NSS")
            })?;
            resize_nss_buffer(&mut buffer, next, &mut reserve).with_context(|| {
                format!("growing {account_kind} NSS buffer for account {name:?}")
            })?;
            continue;
        }
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status))
                .with_context(|| format!("resolving {account_kind} account {name:?} through NSS"));
        }
        if result.is_null() {
            return Ok(None);
        }
        anyhow::ensure!(
            result == output.as_mut_ptr(),
            "{account_kind} NSS lookup for {name:?} returned an unexpected result pointer"
        );
        // SAFETY: a zero return with the exact caller-provided non-null result
        // pointer is the getpwnam_r/getgrnam_r contract that initializes `output`.
        let resolved_id = id(unsafe { output.assume_init_ref() });
        return Ok(Some(resolved_id));
    }
}

fn lookup_uid(name: &str) -> Result<Option<u32>> {
    lookup_account(
        name,
        "user",
        sysconf_buffer_hint(libc::_SC_GETPW_R_SIZE_MAX),
        NSS_BUFFER_LIMITS,
        reserve_nss_buffer,
        |cname, output, buffer, buffer_len, result| {
            // SAFETY: `cname` is NUL-terminated; `output`, `buffer`, and `result`
            // are caller-owned, valid for their stated sizes, and remain live
            // until the return code and exact result pointer are checked.
            unsafe { libc::getpwnam_r(cname.as_ptr(), output, buffer, buffer_len, result) }
        },
        |passwd: &libc::passwd| passwd.pw_uid,
    )
}

fn lookup_gid(name: &str) -> Result<Option<u32>> {
    lookup_account(
        name,
        "group",
        sysconf_buffer_hint(libc::_SC_GETGR_R_SIZE_MAX),
        NSS_BUFFER_LIMITS,
        reserve_nss_buffer,
        |cname, output, buffer, buffer_len, result| {
            // SAFETY: `cname` is NUL-terminated; `output`, `buffer`, and `result`
            // are caller-owned, valid for their stated sizes, and remain live
            // until the return code and exact result pointer are checked.
            unsafe { libc::getgrnam_r(cname.as_ptr(), output, buffer, buffer_len, result) }
        },
        |group: &libc::group| group.gr_gid,
    )
}

trait AccountResolver {
    fn lookup_uid(&mut self, name: &str) -> Result<Option<u32>>;
    fn lookup_gid(&mut self, name: &str) -> Result<Option<u32>>;
}

struct SystemAccountResolver;

impl AccountResolver for SystemAccountResolver {
    fn lookup_uid(&mut self, name: &str) -> Result<Option<u32>> {
        lookup_uid(name)
    }

    fn lookup_gid(&mut self, name: &str) -> Result<Option<u32>> {
        lookup_gid(name)
    }
}

fn derive_instance_owner_with(
    instance: &str,
    resolver: &mut impl AccountResolver,
) -> Result<Option<(u32, u32)>> {
    let inst = instance.trim();
    if inst.is_empty() {
        return Ok(None);
    }
    let name = format!("underwrap-{inst}");
    let Some(uid) = resolver.lookup_uid(&name)? else {
        return Ok(None);
    };
    let Some(gid) = resolver.lookup_gid(&name)? else {
        return Ok(None);
    };
    Ok(Some((uid, gid)))
}

fn resolve_targets_with(cli: &Cli, resolver: &mut impl AccountResolver) -> Result<ResolvedTargets> {
    if let Some(path) = cli.instance_config.as_deref() {
        let yaml = std::fs::read_to_string(path)
            .with_context(|| format!("reading instance config '{}'", path.display()))?;
        let doc: serde_yaml::Value = serde_yaml::from_str(&yaml)
            .with_context(|| format!("parsing instance config '{}'", path.display()))?;
        let backend_value = doc.get("backend").cloned().with_context(|| {
            format!(
                "instance config '{}' has no backend section",
                path.display()
            )
        })?;
        let backend: BackendServiceConfig =
            serde_yaml::from_value(backend_value).with_context(|| {
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
        let credential_owner = match path.file_stem().and_then(|stem| stem.to_str()) {
            Some(instance) => derive_instance_owner_with(instance, resolver)?,
            None => None,
        };
        Ok(ResolvedTargets {
            client_identity_out,
            credential_owner,
        })
    } else {
        let client_identity_out = cli
            .client_identity_out
            .clone()
            .context("either --instance-config or --client-identity-out is required")?;
        Ok(ResolvedTargets {
            client_identity_out,
            credential_owner: None,
        })
    }
}

pub fn run(cli: Cli) -> Result<()> {
    harden_process()?;
    run_after_hardening(cli, &mut SystemAccountResolver, || {
        generate_key_and_self_signed_cert()
    })
}

fn run_after_hardening(
    cli: Cli,
    resolver: &mut impl AccountResolver,
    generate: impl FnOnce() -> Result<(Zeroizing<Vec<u8>>, String)>,
) -> Result<()> {
    let targets = resolve_targets_with(&cli, resolver)?;
    validate_output_path(&targets.client_identity_out)?;

    eprintln!("underwrap-keygen: generating client key and self-signed cert");
    let (identity_pem, cert_pem) = generate()?;

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
    // SAFETY: PR_SET_DUMPABLE takes trusted scalar arguments and changes only
    // calling-process state; the return is checked before key generation.
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
    // SAFETY: `rlim` is initialized, aligned, and live for the call; RLIMIT_CORE
    // is trusted and the return is checked before key generation.
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
        // SAFETY: the temporary file owns this live descriptor; `uid` and `gid`
        // are selected scalar ownership values and the return is checked before persist.
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
