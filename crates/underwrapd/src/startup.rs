use anyhow::{Context, Result};
use std::mem::MaybeUninit;
use tokio::net::UnixListener;
use tokio::task::JoinSet;
use tracing::info;

use underwrap_protocol::config::DaemonConfig;

/// Resolve the single instance group to a GID via caller-owned NSS state at
/// startup. Acquiring this group (setgid on the `underwrap-<instance>` binary)
/// is the sole kernel-enforced gate for the instance's wrapper socket;
/// the wrapper name is a client-supplied selector validated by the
/// daemon, not a second kernel identity.
pub(crate) fn resolve_instance_gid(config: &DaemonConfig) -> Result<u32> {
    let gid = resolve_group_gid(&config.group)
        .with_context(|| format!("resolving instance group '{}'", config.group))?;
    info!(instance = %config.instance, group = %config.group, gid, "resolved instance group");
    Ok(gid)
}

const GROUP_BUFFER_FALLBACK: usize = 16 * 1024;
const GROUP_BUFFER_MAX: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupBufferHint {
    Unavailable,
    Value(i128),
}

#[derive(Clone, Copy, Debug)]
struct GroupBufferLimits {
    fallback: usize,
    max: usize,
}

const GROUP_BUFFER_LIMITS: GroupBufferLimits = GroupBufferLimits {
    fallback: GROUP_BUFFER_FALLBACK,
    max: GROUP_BUFFER_MAX,
};

fn group_buffer_hint() -> GroupBufferHint {
    // SAFETY: `_SC_GETGR_R_SIZE_MAX` requests a scalar group-lookup buffer hint;
    // `sysconf` does not receive or write through any caller-owned pointer.
    let value = unsafe { libc::sysconf(libc::_SC_GETGR_R_SIZE_MAX) };
    if value == -1 {
        GroupBufferHint::Unavailable
    } else {
        GroupBufferHint::Value(i128::from(value))
    }
}

fn initial_group_buffer_size(hint: GroupBufferHint, limits: GroupBufferLimits) -> Result<usize> {
    anyhow::ensure!(
        limits.max <= GROUP_BUFFER_MAX,
        "group NSS buffer limit {} exceeds the {GROUP_BUFFER_MAX}-byte hard maximum",
        limits.max
    );
    let hinted = match hint {
        GroupBufferHint::Unavailable | GroupBufferHint::Value(..=0) => limits.fallback,
        GroupBufferHint::Value(value) => usize::try_from(value)
            .with_context(|| format!("group NSS buffer-size hint {value} is not representable"))?,
    };
    anyhow::ensure!(
        hinted <= limits.max,
        "group NSS buffer-size hint {hinted} exceeds the {}-byte maximum",
        limits.max
    );
    Ok(hinted)
}

fn reserve_group_buffer(buffer: &mut Vec<MaybeUninit<u8>>, additional: usize) -> Result<()> {
    buffer
        .try_reserve_exact(additional)
        .context("allocating caller-owned group NSS lookup buffer")
}

fn resize_group_buffer(
    buffer: &mut Vec<MaybeUninit<u8>>,
    target_len: usize,
    limits: GroupBufferLimits,
    reserve: &mut impl FnMut(&mut Vec<MaybeUninit<u8>>, usize) -> Result<()>,
) -> Result<()> {
    anyhow::ensure!(
        target_len <= limits.max,
        "group NSS buffer request {target_len} exceeds the {}-byte maximum",
        limits.max
    );
    let additional = target_len
        .checked_sub(buffer.len())
        .context("group NSS buffer target length is smaller than its current length")?;
    reserve(buffer, additional)?;
    buffer.resize_with(target_len, MaybeUninit::uninit);
    Ok(())
}

fn next_group_buffer_size(current: usize, max: usize) -> Result<usize> {
    anyhow::ensure!(
        current < max,
        "group NSS lookup exhausted the {max}-byte buffer maximum"
    );
    let doubled = current
        .checked_mul(2)
        .context("group NSS buffer growth arithmetic overflow")?;
    Ok(doubled.min(max))
}

fn resolve_group_gid_with(
    name: &str,
    hint: GroupBufferHint,
    limits: GroupBufferLimits,
    mut reserve: impl FnMut(&mut Vec<MaybeUninit<u8>>, usize) -> Result<()>,
    mut lookup: impl FnMut(
        &std::ffi::CStr,
        *mut libc::group,
        *mut libc::c_char,
        usize,
        *mut *mut libc::group,
    ) -> libc::c_int,
) -> Result<Option<u32>> {
    let c_name = std::ffi::CString::new(name)
        .with_context(|| format!("group name '{name}' contains null byte"))?;
    let initial = initial_group_buffer_size(hint, limits)?;
    let mut buffer = Vec::new();
    resize_group_buffer(&mut buffer, initial, limits, &mut reserve)?;
    let mut output = MaybeUninit::<libc::group>::uninit();

    loop {
        let mut result = std::ptr::null_mut();
        let status = lookup(
            &c_name,
            output.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        );
        if status == libc::ERANGE {
            let next = next_group_buffer_size(buffer.len(), limits.max)
                .with_context(|| format!("resolving group {name:?} through NSS"))?;
            resize_group_buffer(&mut buffer, next, limits, &mut reserve)
                .with_context(|| format!("growing group NSS buffer for {name:?}"))?;
            continue;
        }
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status))
                .with_context(|| format!("resolving group {name:?} through NSS"));
        }
        if result.is_null() {
            return Ok(None);
        }
        anyhow::ensure!(
            result == output.as_mut_ptr(),
            "group NSS lookup for {name:?} returned an unexpected result pointer"
        );
        // SAFETY: a zero return with the exact caller-provided non-null result
        // pointer is the `getgrnam_r` contract that initializes `output`; the
        // caller-owned backing buffer remains live while `gr_gid` is copied.
        let gid = unsafe { output.assume_init_ref().gr_gid };
        return Ok(Some(gid));
    }
}

/// Resolve a Unix group name to a copied GID.
fn resolve_group_gid(name: &str) -> Result<u32> {
    resolve_group_gid_with(
        name,
        group_buffer_hint(),
        GROUP_BUFFER_LIMITS,
        reserve_group_buffer,
        |c_name, output, buffer, buffer_len, result| {
            // SAFETY: `c_name` is NUL-terminated, and `output`, `buffer`, and
            // `result` are caller-owned, valid for the supplied sizes, and live
            // until the status and exact result pointer are validated.
            unsafe { libc::getgrnam_r(c_name.as_ptr(), output, buffer, buffer_len, result) }
        },
    )?
    .ok_or_else(|| anyhow::anyhow!("group '{name}' not found"))
}

/// Parse and validate the runtime backend endpoint without performing any I/O.
/// The returned URL is used only as validation evidence; endpoint construction
/// deliberately retains the daemon's existing base-path concatenation rules.
pub(crate) fn validate_backend_url(value: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| anyhow::anyhow!("backend.url must be an absolute HTTPS URL with a host"))?;
    anyhow::ensure!(
        url.scheme() == "https" && url.host().is_some(),
        "backend.url must be an absolute HTTPS URL with a host"
    );
    Ok(url)
}

/// Validate configuration at load time.
pub(crate) fn validate_config(config: &DaemonConfig) -> Result<()> {
    validate_backend_url(&config.backend.url)?;

    // Instance-level invariants (reserved `debug` name, projection slot
    // presence, non-empty instance name) shared with the backend.
    config.validate().map_err(|e| anyhow::anyhow!(e))?;

    anyhow::ensure!(
        config.socket.is_absolute(),
        "instance socket must be an absolute path, got '{}'",
        config.socket.display(),
    );
    anyhow::ensure!(!config.group.is_empty(), "instance group must be set");

    for (name, wrapper) in &config.wrappers {
        anyhow::ensure!(
            !wrapper.resource.is_empty(),
            "wrapper '{}' must declare a resource",
            name
        );
        if let Some(files) = &wrapper.files {
            anyhow::ensure!(
                !files.paths.is_empty(),
                "wrapper '{name}' files slot has no paths"
            );
            for path in &files.paths {
                validate_relative_credential_path(path)
                    .with_context(|| format!("wrapper '{name}' files slot has invalid path"))?;
            }
            for (i, left) in files.paths.iter().enumerate() {
                for right in files.paths.iter().skip(i + 1) {
                    anyhow::ensure!(
                        !paths_overlap(left, right),
                        "wrapper '{}' uses overlapping files paths '{}' and '{}'",
                        name,
                        left,
                        right,
                    );
                }
            }
        }
    }
    Ok(())
}

/// Refuse to start if an mTLS client credential file exists on disk but
/// is readable beyond the daemon user.
///
/// The daemon self-heals when the identity is absent (the operator hasn't run
/// `underwrap-keygen` yet), so a *missing* file is not an error — only a present file with
/// the wrong owner, an over-permissive mode, or a world/group-accessible
/// parent directory is. This turns a key-leak misconfiguration into a
/// loud startup failure instead of silent operator-discipline reliance.
pub(crate) fn check_credential_permissions(config: &DaemonConfig) -> Result<()> {
    let euid = unsafe { libc::geteuid() };
    let Some(path) = config.backend.client_identity_file.as_ref() else {
        return Ok(());
    };
    if !path.exists() {
        return Ok(());
    }
    let label = "backend client identity";
    let meta =
        std::fs::metadata(path).with_context(|| format!("stat {label} file {}", path.display()))?;
    check_credential_file_meta(label, &path.display().to_string(), &meta, euid)?;

    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty())
        && let Ok(pmeta) = std::fs::metadata(parent)
    {
        check_credential_dir_meta(label, &parent.display().to_string(), &pmeta, euid)?;
    }
    Ok(())
}

/// `0600` or tighter, owned by the daemon's effective uid.
fn check_credential_file_meta(
    label: &str,
    display: &str,
    meta: &std::fs::Metadata,
    euid: u32,
) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;

    anyhow::ensure!(
        meta.uid() == euid,
        "{label} file {display} is owned by uid {} but the daemon runs as uid {euid}; \
         refusing to start (run underwrap-keygen as the daemon user)",
        meta.uid(),
    );
    let mode = meta.permissions().mode() & 0o777;
    anyhow::ensure!(
        mode & !0o600 == 0,
        "{label} file {display} has mode {mode:#o}; must be 0600 or tighter \
         (no group/other access); refusing to start",
    );
    Ok(())
}

/// Parent directory: `0700` or tighter, owned by the daemon's effective uid.
fn check_credential_dir_meta(
    label: &str,
    display: &str,
    meta: &std::fs::Metadata,
    euid: u32,
) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;

    anyhow::ensure!(
        meta.uid() == euid,
        "{label} parent directory {display} is owned by uid {} but the daemon runs as \
         uid {euid}; refusing to start",
        meta.uid(),
    );
    let mode = meta.permissions().mode() & 0o777;
    anyhow::ensure!(
        mode & 0o077 == 0,
        "{label} parent directory {display} has mode {mode:#o}; must be 0700 or tighter \
         (no group/other access); refusing to start",
    );
    Ok(())
}

pub(crate) fn validate_relative_credential_path(path: &str) -> Result<()> {
    anyhow::ensure!(!path.is_empty(), "path must not be empty");
    anyhow::ensure!(!path.starts_with('/'), "path must be relative");
    anyhow::ensure!(!path.contains('\0'), "path must not contain null bytes");
    for component in path.split('/') {
        anyhow::ensure!(
            !component.is_empty(),
            "path must not contain empty segments"
        );
        anyhow::ensure!(component != "." && component != "..", "path must be clean");
    }
    Ok(())
}

pub(crate) fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left
            .strip_prefix(right)
            .is_some_and(|rest| rest.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|rest| rest.starts_with('/'))
}

pub(crate) fn bind_socket(
    path: &std::path::Path,
    mode: u32,
    label: &str,
    expected_gid: Option<u32>,
) -> Result<UnixListener> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {label} socket dir {}", parent.display()))?;
    }
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)
        .with_context(|| format!("binding {label} socket {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .with_context(|| format!("setting {label} socket permissions to {mode:#o}"))?;

        // The 0660 socket is gated on the instance group. We do NOT chown
        // it: the daemon's primary group already *is* the instance group
        // (NixOS module + integration harness both run it that way), so a
        // freshly bound socket inherits that group from the daemon's egid.
        // Rather than re-assert it with a privileged chown (which would
        // force `@chown` back into the systemd syscall filter), verify the
        // kernel-assigned group and fail closed if it is wrong — a
        // wrongly-grouped socket would silently widen or break the gate.
        if let Some(gid) = expected_gid {
            let meta = std::fs::metadata(path)
                .with_context(|| format!("stat {label} socket {} for gid check", path.display()))?;
            anyhow::ensure!(
                meta.gid() == gid,
                "{label} socket {} was created with group gid {} but the instance group is \
                 gid {gid}; the daemon's primary group must be the instance group — refusing \
                 to serve a wrongly-grouped socket",
                path.display(),
                meta.gid(),
            );
        }
    }

    info!(path = %path.display(), mode = format!("{mode:#o}"), "{label} socket listening");
    Ok(listener)
}

/// Wait for the next listener task result; used by the main server loop.
pub(crate) async fn next_listener_result(tasks: &mut JoinSet<Result<()>>) -> Result<()> {
    match tasks.join_next().await {
        Some(result) => result.context("socket listener task panicked")?,
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests;
