use anyhow::Result;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::time::{Duration, timeout};
use tracing::warn;

use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{ErrorCode, WrapperRequest, WrapperResponse};

use crate::audit;
use crate::peer_auth::{PeerCred, get_peer_cred};
use crate::server::State;

const WRAPPER_FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) async fn authorize_wrapper_peer(
    stream: &mut UnixStream,
    _state: &State,
    wrapper_name: &str,
    expected_gid: u32,
) -> Result<Option<(Option<PeerCred>, u32)>> {
    let peer = get_peer_cred(stream);
    let peer_uid = match peer.as_ref().map(|p| p.uid) {
        Some(uid) => uid,
        None => {
            write_frame_async(
                stream,
                &WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: "could not determine peer uid".to_string(),
                },
            )
            .await?;
            stream.shutdown().await?;
            return Ok(None);
        }
    };

    if peer.as_ref().and_then(|p| p.egid) != Some(expected_gid) {
        let reason = format!(
            "peer egid {:?} does not match wrapper '{}' gid {}",
            peer.as_ref().and_then(|p| p.egid),
            wrapper_name,
            expected_gid,
        );
        warn!(wrapper = %wrapper_name, peer = ?peer, "wrapper identity rejected");
        audit::auth_denied(
            "wrapper",
            &reason,
            Some(peer_uid),
            peer.as_ref().map(|p| p.gid),
        );
        write_frame_async(
            stream,
            &WrapperResponse::Error {
                code: ErrorCode::Unauthorized,
                message: reason,
            },
        )
        .await?;
        stream.shutdown().await?;
        return Ok(None);
    }

    Ok(Some((peer, peer_uid)))
}

pub(super) async fn read_initial_wrapper_request(
    stream: &mut UnixStream,
    wrapper_name: &str,
    peer: Option<&PeerCred>,
    peer_uid: u32,
) -> Result<Option<WrapperRequest>> {
    match timeout(WRAPPER_FIRST_FRAME_TIMEOUT, read_frame_async(stream)).await {
        Ok(Ok(request)) => Ok(Some(request)),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            warn!(wrapper = %wrapper_name, peer = ?peer, "wrapper socket first-frame timeout");
            audit::auth_denied(
                "wrapper",
                "timed out waiting for first wrapper request frame",
                Some(peer_uid),
                peer.map(|p| p.gid),
            );
            stream.shutdown().await?;
            Ok(None)
        }
    }
}
