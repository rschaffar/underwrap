use anyhow::Result;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tracing::{info, warn};

use underwrap_protocol::ipc::write_frame_async;
use underwrap_protocol::message::{ErrorCode, WrapperRequest, WrapperResponse};
use underwrap_protocol::variant::WrapperVariant;

use crate::peer_auth::PeerCred;
use crate::server::{BackendOpenError, State};
use crate::wrapper_resource_channel::ChannelContext;

mod channels;
mod intake;
mod read;

use channels::establish_scoped_channels;
use intake::{authorize_wrapper_peer, read_initial_wrapper_request};
use read::open_invocation;

pub(crate) async fn handle_wrapper(
    mut stream: UnixStream,
    state: Arc<State>,
    expected_gid: u32,
) -> Result<()> {
    let (peer, peer_uid) = match authorize_wrapper_peer(&mut stream, expected_gid).await? {
        Some(auth) => auth,
        None => return Ok(()),
    };

    let request = match read_initial_wrapper_request(&mut stream, &peer, peer_uid).await? {
        Some(request) => request,
        None => return Ok(()),
    };

    // The wrapper name is client-supplied now that one socket serves the
    // whole instance; it is validated against the daemon config in
    // `open_invocation`. The variant (Real/Debug) likewise comes from the
    // client, but the daemon — not the client — chooses the target binary
    // and the backend re-evaluates the policy for the variant, so a
    // forged `variant` cannot escalate.
    let (wrapper_name, variant, scope) = match request {
        WrapperRequest::Read {
            wrapper_name,
            variant,
            scope,
        } => (wrapper_name, variant, scope),
    };

    let outcome = open_one_invocation(
        scope.as_deref(),
        &wrapper_name,
        variant,
        &peer,
        peer_uid,
        &state,
    )
    .await;
    write_frame_async(&mut stream, &outcome.response).await?;

    if let Some(channel) = outcome.channel {
        establish_scoped_channels(&stream, &wrapper_name, channel).await;
    }

    stream.shutdown().await?;
    Ok(())
}

/// Outcome of one wrapper Read frame: the wire response to send back,
/// and (on Allow) the per-invocation state needed to set up the
/// scoped wrapper-resource channel.
pub(in crate::wrapper_flow) struct ReadOutcome {
    pub(in crate::wrapper_flow) response: WrapperResponse,
    pub(in crate::wrapper_flow) channel: Option<ChannelContext>,
}

async fn open_one_invocation(
    scope: Option<&str>,
    wrapper_name: &str,
    variant: WrapperVariant,
    peer: &PeerCred,
    peer_uid: u32,
    state: &State,
) -> ReadOutcome {
    info!(wrapper = %wrapper_name, ?variant, peer = ?peer, "wrapper read request");

    let backend = match state.ensure_backend() {
        Ok(b) => b,
        Err(err) => {
            match &err {
                BackendOpenError::MissingCredential => warn!(
                    wrapper = %wrapper_name,
                    "wrapper request denied: client identity is not on disk yet \
                     (run `underwrap-keygen` for this instance)",
                ),
                BackendOpenError::Open(e) => warn!(
                    wrapper = %wrapper_name,
                    err = %format!("{e:#}"),
                    "wrapper request denied: backend client construction failed",
                ),
            }
            return ReadOutcome {
                response: WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: crate::server::unenrolled_message(&state.enrollment_url()),
                },
                channel: None,
            };
        }
    };

    open_invocation(scope, wrapper_name, variant, peer_uid, state, backend).await
}
