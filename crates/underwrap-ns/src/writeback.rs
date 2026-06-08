//! End-of-run decision and dispatch on the wrapper-resource channels.
//!
//! For every resource in scope, ns composes the post-run `ResourceBlob`
//! from (env captured at Checkout, files walked from the staging tmpfs,
//! libsecret read from the on-tmpfs state file) and JCS-canonicalizes
//! it. The byte-comparison against the Checkout's `original_canonical`
//! drives the dispatch:
//!
//! - bytes equal              → `UsageSignal` (no change to record).
//! - bytes differ + writable  → `Checkin { blob }`. On `Unauthorized`
//!   the change is dropped locally; one warning is emitted.
//! - bytes differ + readonly  → drop the change; one warning is
//!   emitted; `UsageSignal` is still sent so the daemon records use.

use std::collections::BTreeMap;
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;

use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{
    ResourceErrorCode, SecretBytes, WrapperResourceRequest, WrapperResourceResponse,
};
use underwrap_protocol::resource_blob::{LibsecretSection, ResourceBlob};

use crate::directory_credential::{DirectoryMountPlan, collect_staging_files};
use crate::resource_channel::ResourceChannel;

/// Per-resource end-of-run outcome, surfaced to the caller so it can
/// emit one journal event plus one optional stderr line per affected
/// resource without re-deriving state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WritebackOutcome {
    /// Bytes unchanged — `UsageSignal` sent.
    Unchanged,
    /// Bytes changed and the daemon accepted the `Checkin`.
    Persisted,
    /// Bytes changed but the daemon rejected the `Checkin` because the
    /// grant is not writable. The change has been dropped.
    RejectedReadOnly {
        resource_id: String,
        resource_name: String,
    },
    /// Bytes changed and the grant was already read-only at Checkout —
    /// no `Checkin` was attempted. The change has been dropped.
    DroppedReadOnly {
        resource_id: String,
        resource_name: String,
    },
}

/// Per-resource input from the run's tmpfs/state side.
pub struct WritebackInput<'a> {
    pub channel: &'a ResourceChannel,
    /// Files-slot layout to walk when composing the post-run blob.
    /// Empty when the wrapper does not project files; in that case the
    /// Checkout's `files` section is preserved verbatim.
    pub directory_layout: &'a [DirectoryMountPlan],
    /// When set, walk the staging with the hardened (openat2) collector
    /// — used for an isolated target, whose staging is target-controlled
    /// and read under a `setfsuid(X)` window.
    pub hardened_walk: bool,
    /// Path to the libsecret state file written by secretsd, or `None`
    /// when this resource is not the libsecret resource. The path is
    /// only consulted when present.
    pub libsecret_state_path: Option<&'a Path>,
}

pub fn dispatch_writeback(inputs: &[WritebackInput<'_>]) -> Result<Vec<WritebackOutcome>, String> {
    let mut outcomes = Vec::with_capacity(inputs.len());
    for input in inputs {
        outcomes.push(dispatch_one(input)?);
    }
    Ok(outcomes)
}

fn dispatch_one(input: &WritebackInput<'_>) -> Result<WritebackOutcome, String> {
    let channel = input.channel;

    let original =
        ResourceBlob::from_canonical_bytes(&channel.original_canonical).map_err(|e| {
            format!(
                "decoding original Checkout blob for '{}': {e}",
                channel.resource_id
            )
        })?;

    let new_files = if input.directory_layout.is_empty() {
        original.files.clone()
    } else {
        collect_staging_files(input.directory_layout, input.hardened_walk).map_err(|e| {
            format!(
                "walking staging directory for '{}': {e}",
                channel.resource_id
            )
        })?
    };

    let new_libsecret = if let Some(path) = input.libsecret_state_path {
        match read_state_no_follow(path) {
            Ok(Some(bytes)) => match LibsecretSection::from_canonical_bytes(&bytes) {
                Ok(section) => section,
                Err(e) => {
                    eprintln!(
                        "underwrap-ns: libsecret state at {} failed to parse ({e}); falling back to Checkout bytes",
                        path.display(),
                    );
                    original.libsecret.clone()
                }
            },
            Ok(None) => original.libsecret.clone(),
            Err(e) => {
                eprintln!(
                    "underwrap-ns: reading libsecret state {} failed: {e}; falling back to Checkout bytes",
                    path.display(),
                );
                original.libsecret.clone()
            }
        }
    } else {
        original.libsecret.clone()
    };

    let new_blob = compose_blob(original.env.clone(), new_files, new_libsecret);
    let new_canonical = new_blob.to_canonical_bytes().map_err(|e| {
        format!(
            "canonicalizing post-run blob for '{}': {e}",
            channel.resource_id
        )
    })?;

    if new_canonical == channel.original_canonical {
        signal_usage(channel.channel_fd, &channel.resource_id)?;
        return Ok(WritebackOutcome::Unchanged);
    }

    if !channel.writable {
        // Don't attempt a Checkin we know the daemon would reject.
        // Still send UsageSignal so the daemon records the run.
        signal_usage(channel.channel_fd, &channel.resource_id)?;
        return Ok(WritebackOutcome::DroppedReadOnly {
            resource_id: channel.resource_id.clone(),
            resource_name: channel.resource_name.clone(),
        });
    }

    match send_checkin(channel.channel_fd, &new_canonical, &channel.resource_id)? {
        CheckinOutcome::Persisted => Ok(WritebackOutcome::Persisted),
        CheckinOutcome::Unauthorized => {
            // Still record use so the daemon's last_used_at advances.
            signal_usage(channel.channel_fd, &channel.resource_id)?;
            Ok(WritebackOutcome::RejectedReadOnly {
                resource_id: channel.resource_id.clone(),
                resource_name: channel.resource_name.clone(),
            })
        }
    }
}

/// Read the libsecret state file refusing to follow a symlink at the final
/// component (`O_NOFOLLOW`) and refusing to block on a planted FIFO
/// (`O_NONBLOCK`). The bus tmpfs is writable by the target (it is uid X for an
/// isolated run), so the state path is target-controllable; the supervisor reads
/// it under a `setfsuid(X)` window, and this is the second layer. Returns
/// `Ok(None)` when the file is absent.
fn read_state_no_follow(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(Some(buf))
}

fn compose_blob(
    env: BTreeMap<String, SecretBytes>,
    files: BTreeMap<String, SecretBytes>,
    libsecret: LibsecretSection,
) -> ResourceBlob {
    let mut blob = ResourceBlob::empty();
    blob.env = env;
    blob.files = files;
    blob.libsecret = libsecret;
    blob
}

enum CheckinOutcome {
    Persisted,
    Unauthorized,
}

fn send_checkin(fd: i32, blob: &[u8], resource_id: &str) -> Result<CheckinOutcome, String> {
    let mut stream = open_stream(fd, resource_id)?;
    write_frame_sync(
        &mut stream,
        &WrapperResourceRequest::Checkin {
            blob: blob.to_vec(),
        },
    )
    .map_err(|e| format!("sending Checkin for '{resource_id}': {e}"))?;
    let resp: WrapperResourceResponse = read_frame_sync(&mut stream)
        .map_err(|e| format!("reading Checkin response for '{resource_id}': {e}"))?;
    match resp {
        WrapperResourceResponse::CheckinOk => Ok(CheckinOutcome::Persisted),
        WrapperResourceResponse::Error {
            code: ResourceErrorCode::Unauthorized,
            ..
        } => Ok(CheckinOutcome::Unauthorized),
        WrapperResourceResponse::Error { code, message } => Err(format!(
            "daemon rejected Checkin for '{resource_id}' ({code:?}): {message}"
        )),
        other => Err(format!(
            "unexpected response to Checkin for '{resource_id}': {other:?}"
        )),
    }
}

fn signal_usage(fd: i32, resource_id: &str) -> Result<(), String> {
    let mut stream = open_stream(fd, resource_id)?;
    write_frame_sync(&mut stream, &WrapperResourceRequest::UsageSignal)
        .map_err(|e| format!("sending UsageSignal for '{resource_id}': {e}"))?;
    let resp: WrapperResourceResponse = read_frame_sync(&mut stream)
        .map_err(|e| format!("reading UsageSignal response for '{resource_id}': {e}"))?;
    match resp {
        WrapperResourceResponse::UsageSignalOk => Ok(()),
        WrapperResourceResponse::Error { code, message } => Err(format!(
            "daemon rejected UsageSignal for '{resource_id}' ({code:?}): {message}"
        )),
        other => Err(format!(
            "unexpected response to UsageSignal for '{resource_id}': {other:?}"
        )),
    }
}

fn open_stream(fd: i32, resource_id: &str) -> Result<UnixStream, String> {
    let dup_fd = unsafe { libc::dup(fd) };
    if dup_fd < 0 {
        return Err(format!(
            "dup({fd}) for '{resource_id}' failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(unsafe { UnixStream::from_raw_fd(dup_fd) })
}
