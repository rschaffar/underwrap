use anyhow::{Context, Result};
use tokio::net::UnixStream;
use tracing::{error, info};

use crate::wrapper_resource_channel::{ChannelContext, run_channel};

/// Set up the single wrapper-resource channel for the resource the
/// backend authorized. Exactly one fd is sent via SCM_RIGHTS,
/// immediately after the `ReadOk` response itself. The channel
/// handler serves `Checkout` / `Checkin` / `UsageSignal` against the
/// backend on behalf of the wrapper.
pub(super) async fn establish_scoped_channels(
    stream: &UnixStream,
    wrapper_name: &str,
    ctx: ChannelContext,
) {
    match setup_wrapper_resource_channel(stream, ctx).await {
        Ok(()) => info!(wrapper = %wrapper_name, "wrapper-resource channel established"),
        Err(e) => error!(
            wrapper = %wrapper_name,
            err = %format!("{e:#}"),
            "failed to setup wrapper-resource channel",
        ),
    }
}

async fn setup_wrapper_resource_channel(stream: &UnixStream, ctx: ChannelContext) -> Result<()> {
    use std::os::fd::AsFd;

    let (daemon_end, wrapper_end) =
        underwrap_protocol::scm_rights::create_socketpair().context("creating socketpair")?;

    underwrap_protocol::scm_rights::send_fd(stream.as_fd(), wrapper_end.as_fd())
        .context("sending wrapper-resource channel fd")?;
    drop(wrapper_end);

    daemon_end.set_nonblocking(true)?;
    let daemon_stream = UnixStream::from_std(daemon_end)?;
    tokio::spawn(async move {
        run_channel(daemon_stream, ctx).await;
    });
    Ok(())
}
