use anyhow::{Context, Result};
use tokio::net::UnixListener;
use tokio::task::JoinSet;
use tracing::info;

use underwrap_protocol::config::DaemonConfig;

/// Resolve the single instance group to a GID via libc getgrnam at
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

/// Resolve a Unix group name to a GID.
pub(crate) fn resolve_group_gid(name: &str) -> Result<u32> {
    let c_name = std::ffi::CString::new(name)
        .with_context(|| format!("group name '{name}' contains null byte"))?;
    let grp = unsafe { libc::getgrnam(c_name.as_ptr()) };
    if grp.is_null() {
        anyhow::bail!("group '{name}' not found");
    }
    Ok(unsafe { (*grp).gr_gid })
}

/// Validate configuration at load time.
pub(crate) fn validate_config(config: &DaemonConfig) -> Result<()> {
    anyhow::ensure!(!config.backend.url.is_empty(), "backend.url must be set");
    anyhow::ensure!(
        config.backend.client_cert_file.is_some() == config.backend.client_key_file.is_some(),
        "backend.client_cert_file and backend.client_key_file must either both be set or both be omitted"
    );

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
        anyhow::ensure!(
            crate::wrapper_bindings::wrapper_has_projection(wrapper),
            "wrapper '{}' must enable at least one projection slot (env, files, or libsecret)",
            name,
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
/// The daemon self-heals when the credential is absent (the operator
/// hasn't run `underwrap-keygen` yet, or the backend hasn't issued a
/// cert), so a *missing* file is not an error — only a present file with
/// the wrong owner, an over-permissive mode, or a world/group-accessible
/// parent directory is. This turns a key-leak misconfiguration into a
/// loud startup failure instead of silent operator-discipline reliance.
pub(crate) fn check_credential_permissions(config: &DaemonConfig) -> Result<()> {
    let euid = unsafe { libc::geteuid() };
    let entries = [
        (
            "backend client cert",
            config.backend.client_cert_file.as_ref(),
        ),
        (
            "backend client key",
            config.backend.client_key_file.as_ref(),
        ),
    ];
    for (label, path) in entries {
        let Some(path) = path else { continue };
        if !path.exists() {
            continue;
        }
        let meta = std::fs::metadata(path)
            .with_context(|| format!("stat {label} file {}", path.display()))?;
        check_credential_file_meta(label, &path.display().to_string(), &meta, euid)?;

        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty())
            && let Ok(pmeta) = std::fs::metadata(parent)
        {
            check_credential_dir_meta(label, &parent.display().to_string(), &pmeta, euid)?;
        }
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
