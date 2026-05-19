use anyhow::{Context, Result};
use tokio::net::UnixStream;
use tracing::{error, info};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::message::WrapperResponse;
use underwrap_protocol::resource::{files_prefix, libsecret_prefix};

use crate::server::State;
use crate::wrapper_bindings::wrapper_file_bindings;

use super::read::BundleSnapshot;

/// Set up scoped channels for resource slots that were already authorized in
/// [`super::read::process_read_for`]. Only called when the response is
/// `ReadOk`, so no re-authorization is needed for initial reads. Write policy
/// remains enforced by each channel's access mode, derived from backend policy.
pub(super) async fn establish_scoped_channels(
    stream: &UnixStream,
    response: &WrapperResponse,
    wrapper_name: &str,
    state: &State,
    snapshot: &BundleSnapshot,
    libsecret_access: Option<&(String, AccessMode)>,
) {
    let WrapperResponse::ReadOk { directories, .. } = response else {
        return;
    };

    let Some(wrapper_def) = state.config.wrappers.get(wrapper_name) else {
        return;
    };

    if let Some((resource_id, access)) = libsecret_access {
        match setup_secretsd_channel(stream, wrapper_name, resource_id, *access, state, snapshot)
            .await
        {
            Ok(()) => {
                info!(wrapper = %wrapper_name, resource = %resource_id, "secretsd channel established");
            }
            Err(e) => {
                error!(wrapper = %wrapper_name, resource = %resource_id, err = %e, "failed to setup secretsd channel");
            }
        }
    }

    for projection in wrapper_file_bindings(wrapper_def) {
        let access = directories
            .iter()
            .find(|binding| binding.resource_id == projection.resource_id)
            .map(|binding| binding.access)
            .unwrap_or(AccessMode::ReadOnly);
        match setup_directory_channel(
            stream,
            wrapper_name,
            &projection.resource_id,
            access,
            state,
            snapshot,
        )
        .await
        {
            Ok(()) => {
                info!(wrapper = %wrapper_name, resource = %projection.resource_id, paths = ?projection.target_paths, "directory channel established");
            }
            Err(e) => {
                error!(wrapper = %wrapper_name, resource = %projection.resource_id, paths = ?projection.target_paths, err = %e, "failed to setup directory channel");
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

async fn setup_secretsd_channel(
    stream: &UnixStream,
    wrapper_name: &str,
    resource_id: &str,
    access: AccessMode,
    state: &State,
    snapshot: &BundleSnapshot,
) -> Result<()> {
    let prefix = libsecret_prefix(resource_id);
    let entries = snapshot.entries.clone();
    let store = state.store_arc();
    let wrapper = wrapper_name.to_string();
    let resource = resource_id.to_string();
    setup_scoped_channel(stream, "secretsd channel fd", move |daemon_stream| {
        tokio::spawn(async move {
            crate::secretsd_channel::run_channel(
                daemon_stream,
                store,
                prefix,
                entries,
                wrapper,
                resource,
                access,
            )
            .await;
        });
    })
    .await
}

async fn setup_directory_channel(
    stream: &UnixStream,
    wrapper_name: &str,
    resource_id: &str,
    access: AccessMode,
    state: &State,
    snapshot: &BundleSnapshot,
) -> Result<()> {
    let prefix = files_prefix(resource_id);
    let entries = snapshot.entries.clone();
    let store = state.store_arc();
    let wrapper = wrapper_name.to_string();
    let resource = resource_id.to_string();
    setup_scoped_channel(stream, "directory channel fd", move |daemon_stream| {
        tokio::spawn(async move {
            crate::directory_channel::run_channel(
                daemon_stream,
                store,
                prefix,
                entries,
                wrapper,
                resource,
                access,
            )
            .await;
        });
    })
    .await
}
