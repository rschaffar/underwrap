use anyhow::{Context, Result};
use tokio::net::UnixStream;
use tracing::{error, info};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::message::WrapperResponse;
use underwrap_protocol::policy::PolicyDecision;

use crate::policy::{authorize_after_approval, directory_resource, libsecret_resource};
use crate::server::State;
use crate::wrapper_bindings::{wrapper_directory_bindings, wrapper_libsecret_binding};

pub(super) async fn establish_scoped_channels(
    stream: &UnixStream,
    response: &WrapperResponse,
    wrapper_name: &str,
    state: &State,
) {
    if !matches!(response, WrapperResponse::ReadOk { .. }) {
        return;
    }

    let Some(wrapper_def) = state.config.wrappers.get(wrapper_name) else {
        return;
    };

    if let Some((credential_ref, access)) =
        wrapper_libsecret_binding(wrapper_def, &state.config.credentials)
    {
        let resource = libsecret_resource(credential_ref);
        let operation = match access {
            AccessMode::ReadOnly => "libsecret.read",
            AccessMode::ReadWrite => "libsecret.readwrite",
        };
        match authorize_after_approval(&*state.store, wrapper_name, &resource, operation) {
            Ok(PolicyDecision::Allow) => {}
            Ok(PolicyDecision::Deny { reason }) => {
                error!(wrapper = %wrapper_name, credential = %credential_ref, reason = %reason, "backend policy denied libsecret channel");
                return;
            }
            Ok(PolicyDecision::NeedsApproval { .. }) => {
                error!(wrapper = %wrapper_name, credential = %credential_ref, "libsecret channel unexpectedly required approval after read");
                return;
            }
            Err(e) => {
                error!(wrapper = %wrapper_name, credential = %credential_ref, err = %e, "backend policy lookup failed for libsecret channel");
                return;
            }
        }
        match setup_secretsd_channel(stream, credential_ref, access, state).await {
            Ok(()) => {
                info!(wrapper = %wrapper_name, credential = %credential_ref, "secretsd channel established");
            }
            Err(e) => {
                error!(wrapper = %wrapper_name, credential = %credential_ref, err = %e, "failed to setup secretsd channel");
            }
        }
    }

    for binding in wrapper_directory_bindings(wrapper_def, &state.config.credentials) {
        let resource = directory_resource(&binding.credential_ref);
        let operation = match binding.access {
            AccessMode::ReadOnly => "directory.read",
            AccessMode::ReadWrite => "directory.readwrite",
        };
        match authorize_after_approval(&*state.store, wrapper_name, &resource, operation) {
            Ok(PolicyDecision::Allow) => {}
            Ok(PolicyDecision::Deny { reason }) => {
                error!(wrapper = %wrapper_name, credential = %binding.credential_ref, path = %binding.target_path, reason = %reason, "backend policy denied directory channel");
                continue;
            }
            Ok(PolicyDecision::NeedsApproval { .. }) => {
                error!(wrapper = %wrapper_name, credential = %binding.credential_ref, path = %binding.target_path, "directory channel unexpectedly required approval after read");
                continue;
            }
            Err(e) => {
                error!(wrapper = %wrapper_name, credential = %binding.credential_ref, path = %binding.target_path, err = %e, "backend policy lookup failed for directory channel");
                continue;
            }
        }
        match setup_directory_channel(stream, &binding.credential_ref, binding.access, state).await
        {
            Ok(()) => {
                info!(wrapper = %wrapper_name, credential = %binding.credential_ref, path = %binding.target_path, "directory channel established");
            }
            Err(e) => {
                error!(wrapper = %wrapper_name, credential = %binding.credential_ref, path = %binding.target_path, err = %e, "failed to setup directory channel");
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
    credential_ref: &str,
    access: AccessMode,
    state: &State,
) -> Result<()> {
    let prefix = credential_ref.to_string();
    let store = state.store_arc();
    let name = credential_ref.to_string();
    setup_scoped_channel(stream, "secretsd channel fd", move |daemon_stream| {
        tokio::spawn(async move {
            crate::secretsd_channel::run_channel(daemon_stream, store, prefix, name, access).await;
        });
    })
    .await
}

async fn setup_directory_channel(
    stream: &UnixStream,
    credential_ref: &str,
    access: AccessMode,
    state: &State,
) -> Result<()> {
    let prefix = format!("{credential_ref}/files");
    let store = state.store_arc();
    let credential = credential_ref.to_string();
    setup_scoped_channel(stream, "directory channel fd", move |daemon_stream| {
        tokio::spawn(async move {
            crate::directory_channel::run_channel(daemon_stream, store, prefix, credential, access)
                .await;
        });
    })
    .await
}
