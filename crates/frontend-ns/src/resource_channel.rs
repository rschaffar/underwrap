//! Per-resource wrapper-resource channel state held by frontend-ns.
//!
//! On startup, ns owns one bulk-blob channel fd per resource in scope
//! (fd 20+i, sent by frontend-exec via dup2). ns sends one `Checkout`
//! on each channel to capture the current JCS-canonical blob plus
//! the grant's writability for the end-of-run decision.
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

/// One resource's state captured at Checkout, used by the end-of-run
/// writeback decision. The fd stays open until the namespace tears
/// down; each IPC op `dup`s it into a temporary `UnixStream`.
#[allow(dead_code)] // some fields consumed in story 410 task 413 cutover.
pub struct ResourceChannel {
    pub resource_id: String,
    pub channel_fd: i32,
    pub original_canonical: Vec<u8>,
    pub writable: bool,
    pub libsecret: bool,
    pub files_target_paths: Vec<String>,
}

/// Drive `Checkout` on every wrapper-resource channel the daemon
/// established for this run. Each channel returns the resource's
/// current JCS-canonical blob plus the writability flag derived from
/// the policy grant.
///
/// Failure on any one channel is fatal — security model is no
/// fallback. The error message names the resource.
pub fn checkout_all(bindings: &[ExecResourceBinding]) -> Result<Vec<ResourceChannel>, String> {
    let mut channels = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let (original_canonical, writable) =
            send_checkout(binding.channel_fd as i32).map_err(|e| {
                format!(
                    "wrapper-resource Checkout for '{}' failed: {e}",
                    binding.resource_id
                )
            })?;
        channels.push(ResourceChannel {
            resource_id: binding.resource_id.clone(),
            channel_fd: binding.channel_fd as i32,
            original_canonical,
            writable,
            libsecret: binding.libsecret,
            files_target_paths: binding.files_target_paths.clone(),
        });
    }
    Ok(channels)
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

/// Drop the wrapper-resource channel fds before exec'ing the target.
/// The target must never inherit the daemon-facing IPC; if it did,
/// a compromised target could replay Checkin against the daemon.
pub fn close_channel_fds_for_exec(channels: &[ResourceChannel]) {
    for channel in channels {
        unsafe {
            libc::close(channel.channel_fd);
        }
    }
}
