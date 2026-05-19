//! End-of-run dispatch on the wrapper-resource channels.
//!
//! The full decision tree (Checkin / UsageSignal / drop+warn from
//! ticket 413) lands once secretsd is fed via pipe and the legacy
//! per-call IPC is removed. This module currently dispatches
//! `UsageSignal` only, which is enough to exercise the channel
//! end-to-end and let the daemon record use without sending a blob.

use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixStream;

use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{WrapperResourceRequest, WrapperResourceResponse};

use crate::resource_channel::ResourceChannel;

pub fn signal_usage_for_all(channels: &[ResourceChannel]) -> Result<(), String> {
    for channel in channels {
        if let Err(e) = signal_usage(channel.channel_fd) {
            return Err(format!(
                "wrapper-resource UsageSignal for '{}' failed: {e}",
                channel.resource_id
            ));
        }
    }
    Ok(())
}

fn signal_usage(fd: i32) -> Result<(), String> {
    let dup_fd = unsafe { libc::dup(fd) };
    if dup_fd < 0 {
        return Err(format!(
            "dup({fd}) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut stream = unsafe { UnixStream::from_raw_fd(dup_fd) };
    write_frame_sync(&mut stream, &WrapperResourceRequest::UsageSignal)
        .map_err(|e| format!("sending UsageSignal: {e}"))?;
    let resp: WrapperResourceResponse =
        read_frame_sync(&mut stream).map_err(|e| format!("reading UsageSignalOk: {e}"))?;
    match resp {
        WrapperResourceResponse::UsageSignalOk => Ok(()),
        WrapperResourceResponse::Error { code, message } => {
            Err(format!("daemon rejected UsageSignal ({code:?}): {message}"))
        }
        other => Err(format!("unexpected UsageSignal response: {other:?}")),
    }
}
