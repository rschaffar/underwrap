//! Wrapper-resource channel state held by frontend-ns.
//!
//! On startup, ns owns the bulk-blob channel fd for the wrapper's one
//! resource (fd 20, sent by frontend-exec via dup2). ns sends a
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

use underwrap_protocol::exec::ExecResourceBinding;
use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
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
    let (original_canonical, writable) = send_checkout(binding.channel_fd as i32).map_err(|e| {
        format!(
            "wrapper-resource Checkout for '{}' failed: {e}",
            binding.resource_id
        )
    })?;
    Ok(ResourceChannel {
        resource_id: binding.resource_id.clone(),
        resource_name: binding.resource_name.clone(),
        channel_fd: binding.channel_fd as i32,
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
    write_frame_sync(&mut stream, &WrapperResourceRequest::Checkout)
        .map_err(|e| format!("sending Checkout: {e}"))?;
    let resp: WrapperResourceResponse =
        read_frame_sync(&mut stream).map_err(|e| format!("reading CheckoutOk: {e}"))?;
    match resp {
        WrapperResourceResponse::CheckoutOk { blob, writable } => Ok((blob, writable)),
        WrapperResourceResponse::Error { code, message } => {
            Err(format!("daemon rejected Checkout ({code:?}): {message}"))
        }
        other => Err(format!("unexpected Checkout response: {other:?}")),
    }
}

/// Drop the wrapper-resource channel fd before exec'ing the target.
/// The target must never inherit the daemon-facing IPC; if it did,
/// a compromised target could replay Checkin against the daemon.
pub fn close_channel_fd_for_exec(channel: &ResourceChannel) {
    unsafe {
        libc::close(channel.channel_fd);
    }
}
