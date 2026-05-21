use anyhow::Result;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tracing::{info, warn};

use underwrap_protocol::ipc::write_frame_async;
use underwrap_protocol::message::{ErrorCode, WrapperRequest, WrapperResponse};

use crate::peer_auth::PeerCred;
use crate::server::{BackendOpenError, State};

mod channels;
mod intake;
mod read;

use channels::establish_scoped_channels;
use intake::{authorize_wrapper_peer, read_initial_wrapper_request};
use read::open_invocation;

pub(crate) async fn handle_wrapper(
    mut stream: UnixStream,
    state: Arc<State>,
    wrapper_name: &str,
    expected_gid: u32,
) -> Result<()> {
    let (peer, peer_uid) =
        match authorize_wrapper_peer(&mut stream, wrapper_name, expected_gid).await? {
            Some(auth) => auth,
            None => return Ok(()),
        };

    let request =
        match read_initial_wrapper_request(&mut stream, wrapper_name, peer.as_ref(), peer_uid)
            .await?
        {
            Some(request) => request,
            None => return Ok(()),
        };

    let scope = match &request {
        WrapperRequest::Read { scope } => scope.clone(),
    };

    let outcome = open_one_invocation(
        scope.as_deref(),
        wrapper_name,
        peer.as_ref(),
        peer_uid,
        &state,
    )
    .await;
    write_frame_async(&mut stream, &outcome.response).await?;

    if let Some(opened) = outcome.opened {
        establish_scoped_channels(&stream, wrapper_name, opened).await;
    }

    stream.shutdown().await?;
    Ok(())
}

/// Outcome of one wrapper Read frame: the wire response to send back,
/// and (on Allow) the per-invocation state needed to set up the
/// scoped wrapper-resource channel.
pub(in crate::wrapper_flow) struct ReadOutcome {
    pub(in crate::wrapper_flow) response: WrapperResponse,
    pub(in crate::wrapper_flow) opened: Option<read::OpenedInvocation>,
}

async fn open_one_invocation(
    scope: Option<&str>,
    wrapper_name: &str,
    peer: Option<&PeerCred>,
    peer_uid: u32,
    state: &State,
) -> ReadOutcome {
    info!(wrapper = %wrapper_name, peer = ?peer, "wrapper read request");

    let backend = match state.ensure_backend() {
        Ok(b) => b,
        Err(err) => {
            match &err {
                BackendOpenError::MissingCredential => warn!(
                    wrapper = %wrapper_name,
                    "wrapper request denied: client credential is not on disk yet \
                     (run `underwrap-keygen` for this instance)",
                ),
                BackendOpenError::Open(e) => warn!(
                    wrapper = %wrapper_name,
                    err = %format!("{e:#}"),
                    "wrapper request denied: backend rejected this client's credential or is unreachable",
                ),
            }
            return ReadOutcome {
                response: WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: crate::server::unenrolled_message(&state.enrollment_url()),
                },
                opened: None,
            };
        }
    };

    open_invocation(scope, wrapper_name, peer_uid, state, backend)
}
