use anyhow::{Context, Result};
use tokio::net::UnixStream;
use tracing::{error, info};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::message::WrapperResponse;

use crate::server::State;

/// Set up the wrapper-resource channels for every resource the daemon
/// authorized in `process_read_for`. Only called when the response is
/// `ReadOk`. Each channel handler enforces the per-resource access mode
/// on `Checkin` and serves `Checkout` / `UsageSignal` unconditionally.
///
/// The send order on the wire is the order the wrapper expects:
/// exactly one fd per `resources` entry in `WrapperResponse::ReadOk`,
/// in the same order.
pub(super) async fn establish_scoped_channels(
    stream: &UnixStream,
    response: &WrapperResponse,
    wrapper_name: &str,
    state: &State,
    resource_access: &[(String, AccessMode)],
) {
    let WrapperResponse::ReadOk { .. } = response else {
        return;
    };

    for (resource_id, access) in resource_access {
        match setup_wrapper_resource_channel(stream, wrapper_name, resource_id, *access, state)
            .await
        {
            Ok(()) => {
                info!(wrapper = %wrapper_name, resource = %resource_id, ?access, "wrapper-resource channel established");
            }
            Err(e) => {
                error!(wrapper = %wrapper_name, resource = %resource_id, err = %e, "failed to setup wrapper-resource channel");
            }
        }
    }
}

async fn setup_scoped_channel(
    stream: &UnixStream,
    label: &str,
    spawn: impl FnOnce(UnixStream) + Send + 'static,
) -> Result<()> {
    use std::os::unix::io::{AsRawFd, FromRawFd};

    let (daemon_end, wrapper_end) =
        underwrap_protocol::scm_rights::create_socketpair().context("creating socketpair")?;

    let std_stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(stream.as_raw_fd()) };
    let wrapper_fd = wrapper_end.as_raw_fd();
    let send_result = underwrap_protocol::scm_rights::send_fd(&std_stream, wrapper_fd);
    std::mem::forget(std_stream);
    send_result.with_context(|| format!("sending {label} fd"))?;
    drop(wrapper_end);

    daemon_end.set_nonblocking(true)?;
    let daemon_stream = UnixStream::from_std(daemon_end)?;
    spawn(daemon_stream);
    Ok(())
}

async fn setup_wrapper_resource_channel(
    stream: &UnixStream,
    wrapper_name: &str,
    resource_id: &str,
    access: AccessMode,
    state: &State,
) -> Result<()> {
    let store = state.store_arc();
    let logical_name = state
        .config
        .wrappers
        .get(wrapper_name)
        .and_then(|def| def.logical_name.clone())
        .unwrap_or_else(|| wrapper_name.to_string());
    let ctx = crate::wrapper_resource_channel::ChannelContext {
        wrapper_name: wrapper_name.to_string(),
        resource_id: resource_id.to_string(),
        access,
        client_instance: state.client_instance.clone(),
        logical_name,
    };
    setup_scoped_channel(
        stream,
        "wrapper-resource channel fd",
        move |daemon_stream| {
            tokio::spawn(async move {
                crate::wrapper_resource_channel::run_channel(daemon_stream, store, ctx).await;
            });
        },
    )
    .await
}
