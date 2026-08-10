//! Wrapper-resource channel state held by underwrap-ns.
//!
//! On startup, ns owns the bulk-blob channel fd for the wrapper's one
//! resource (fd 20, sent by underwrap-exec via dup2). ns sends a
//! `Checkout` on the channel to capture the current JCS-canonical blob
//! plus the grant's writability for the end-of-run decision.
//!
//! The fd is held open as a raw integer rather than a `UnixStream`,
//! mirroring the `DirectoryChannel` pattern: each operation `dup`s
//! the fd into a short-lived `UnixStream` and drops it, so the
//! original fd stays open across `fork()` and is closed exactly once
//! in the child (via `libc::close`) before exec.

use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixStream;

use underwrap_protocol::exec::{ExecResourceBinding, WRAPPER_RESOURCE_CHANNEL_FD};
use underwrap_protocol::ipc::{
    read_wrapper_resource_frame_sync, write_wrapper_resource_frame_sync,
};
use underwrap_protocol::message::{WrapperResourceRequest, WrapperResourceResponse};

/// The resource's state captured at Checkout, used by the end-of-run
/// writeback decision. The fd stays open until the namespace tears
/// down; each IPC op `dup`s it into a temporary `UnixStream`.
#[allow(dead_code)] // some fields consumed later in the wrapper flow.
pub struct ResourceChannel {
    pub resource_id: String,
    pub resource_name: String,
    pub channel_fd: i32,
    pub original_canonical: Vec<u8>,
    pub writable: bool,
    pub libsecret: bool,
    pub files_target_paths: Vec<String>,
}

/// Drive `Checkout` on the wrapper's resource channel. Returns the
/// resource's current JCS-canonical blob plus the writability flag
/// derived from the policy grant.
///
/// Failure is fatal — security model is no fallback. The error message
/// names the resource.
pub fn checkout(binding: &ExecResourceBinding) -> Result<ResourceChannel, String> {
    let (original_canonical, writable) =
        send_checkout(WRAPPER_RESOURCE_CHANNEL_FD).map_err(|e| {
            format!(
                "wrapper-resource Checkout for '{}' failed: {e}",
                binding.resource_id
            )
        })?;
    Ok(ResourceChannel {
        resource_id: binding.resource_id.clone(),
        resource_name: binding.resource_name.clone(),
        channel_fd: WRAPPER_RESOURCE_CHANNEL_FD,
        original_canonical,
        writable,
        libsecret: binding.libsecret,
        files_target_paths: binding.files_target_paths.clone(),
    })
}

fn send_checkout(fd: i32) -> Result<(Vec<u8>, bool), String> {
    let dup_fd = unsafe { libc::dup(fd) };
    if dup_fd < 0 {
        return Err(format!(
            "dup({fd}) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut stream = unsafe { UnixStream::from_raw_fd(dup_fd) };
    write_wrapper_resource_frame_sync(&mut stream, &WrapperResourceRequest::Checkout)
        .map_err(|e| format!("sending Checkout: {e}"))?;
    let resp: WrapperResourceResponse = read_wrapper_resource_frame_sync(&mut stream)
        .map_err(|e| format!("reading CheckoutOk: {e}"))?;
    match resp {
        WrapperResourceResponse::CheckoutOk { blob, writable } => Ok((blob, writable)),
        WrapperResourceResponse::Error { code, message } => {
            Err(format!("daemon rejected Checkout ({code:?}): {message}"))
        }
        other => Err(format!("unexpected Checkout response: {other:?}")),
    }
}

/// Close this process's inherited wrapper-resource channel endpoint. Parent
/// and namespace child each call this once after `fork`; the numeric descriptor
/// is never retried after `close(2)`, whose error leaves ownership ambiguous.
pub fn close_channel_fd(channel: &ResourceChannel) -> Result<(), String> {
    // SAFETY: fd 20 is the authenticated endpoint owned by this process. After
    // this call no code in the same process uses the descriptor again.
    if unsafe { libc::close(channel.channel_fd) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}
