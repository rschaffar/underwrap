//! End-of-run decision and dispatch on the wrapper-resource channels.
//!
//! For every resource in scope, ns composes the post-run `ResourceBlob`
//! from env captured at Checkout, files walked from staging tmpfs, and the
//! host supervisor's latest validated libsecret generation, then canonicalizes
//! it. The byte-comparison against the Checkout's `original_canonical`
//! drives the dispatch:
//!
//! - bytes equal              → `UsageSignal` (no change to record).
//! - bytes differ + writable  → `Checkin { blob }`. On `Unauthorized`
//!   the change is dropped locally; one warning is emitted.
//! - bytes differ + readonly  → drop the change; one warning is
//!   emitted; `UsageSignal` is still sent so the daemon records use.
//! - complete composition too large → drop the change with attempted/max
//!   diagnostics and send no terminal resource verb. The caller retains the
//!   already-authoritative target status.

use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixStream;
use underwrap_protocol::ipc::{
    read_wrapper_resource_frame_sync, write_wrapper_resource_frame_sync,
};
use underwrap_protocol::message::{
    ResourceErrorCode, WrapperResourceRequest, WrapperResourceResponse,
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
    /// Bytes changed but backend finalize rejected the `Checkin` after
    /// re-evaluating effective backend access and writability. The change has
    /// been dropped.
    RejectedReadOnly {
        resource_id: String,
        resource_name: String,
    },
    /// Bytes changed and the effective writable flag was already false
    /// at Checkout — no `Checkin` was attempted. The change has been dropped.
    DroppedReadOnly {
        resource_id: String,
        resource_name: String,
    },
    /// Bytes changed but composing the complete canonical resource exceeded
    /// the public size authority. No terminal resource verb was attempted.
    RejectedTooLarge {
        resource_id: String,
        resource_name: String,
        attempted_len: usize,
        maximum_len: usize,
    },
}

/// Per-resource input from the run's tmpfs/state side.
pub struct WritebackInput<'a> {
    pub channel: &'a ResourceChannel,
    /// Files-slot layout to walk when composing the post-run blob.
    /// Empty when the wrapper does not project files; in that case the
    /// Checkout's `files` section is preserved verbatim. For an isolated
    /// target this walk runs inside a `setfsuid(X)` window (see
    /// [`crate::run_parent`]), which bounds the cross-uid read to uid X.
    pub directory_layout: &'a [DirectoryMountPlan],
    /// Latest complete host-validated generation whose exact ACK was fully
    /// written, or `None` when this resource does not project libsecret.
    pub libsecret_generation: Option<&'a LibsecretSection>,
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
        collect_staging_files(input.directory_layout).map_err(|e| {
            format!(
                "walking staging directory for '{}': {e}",
                channel.resource_id
            )
        })?
    };

    let new_libsecret = input
        .libsecret_generation
        .cloned()
        .unwrap_or_else(|| original.libsecret.clone());

    let mut new_blob = original;
    new_blob.files = new_files;
    new_blob.libsecret = new_libsecret;
    let new_canonical = match new_blob.to_canonical_bytes() {
        Ok(bytes) => bytes,
        Err(error) => {
            if let Some(size) = underwrap_protocol::resource_blob::resource_too_large_error(&error)
            {
                return Ok(WritebackOutcome::RejectedTooLarge {
                    resource_id: channel.resource_id.clone(),
                    resource_name: channel.resource_name.clone(),
                    attempted_len: size.attempted_len(),
                    maximum_len: size.maximum_len(),
                });
            }
            return Err(format!(
                "canonicalizing post-run blob for '{}': {error}",
                channel.resource_id
            ));
        }
    };

    if new_canonical == channel.original_canonical {
        signal_usage(channel.channel_fd, &channel.resource_id)?;
        return Ok(WritebackOutcome::Unchanged);
    }

    if !channel.writable {
        // Honor the open-time read-only hint, even if backend settings changed.
        // UsageSignal is still subject to current backend reauthorization.
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

enum CheckinOutcome {
    Persisted,
    Unauthorized,
}

fn send_checkin(fd: i32, blob: &[u8], resource_id: &str) -> Result<CheckinOutcome, String> {
    let mut stream = open_stream(fd, resource_id)?;
    write_wrapper_resource_frame_sync(
        &mut stream,
        &WrapperResourceRequest::Checkin {
            blob: blob.to_vec(),
        },
    )
    .map_err(|e| format!("sending Checkin for '{resource_id}': {e}"))?;
    let resp: WrapperResourceResponse = read_wrapper_resource_frame_sync(&mut stream)
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
    write_wrapper_resource_frame_sync(&mut stream, &WrapperResourceRequest::UsageSignal)
        .map_err(|e| format!("sending UsageSignal for '{resource_id}': {e}"))?;
    let resp: WrapperResourceResponse = read_wrapper_resource_frame_sync(&mut stream)
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
    // SAFETY: `fd` is the live authenticated resource-channel endpoint; dup
    // borrows it and returns either an error or one fresh owned descriptor.
    let dup_fd = unsafe { libc::dup(fd) };
    if dup_fd < 0 {
        return Err(format!(
            "dup({fd}) for '{resource_id}' failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: successful dup returned this nonnegative descriptor with unique
    // ownership, transferred exactly once into UnixStream RAII.
    Ok(unsafe { UnixStream::from_raw_fd(dup_fd) })
}
