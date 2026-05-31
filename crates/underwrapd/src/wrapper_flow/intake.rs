use anyhow::Result;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::time::{Duration, timeout};
use tracing::warn;

use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{ErrorCode, WrapperRequest, WrapperResponse};

use crate::audit;
use crate::peer_auth::{PeerAuthReject, PeerCred, get_peer_cred};

const WRAPPER_FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(2);

// Wire error returned for every authentication failure. Kept deliberately
// generic so the peer cannot learn which check rejected it; the specific
// reason goes to the structured audit log only.
const PEER_AUTH_WIRE_ERROR: &str = "peer authentication failed";

pub(super) async fn authorize_wrapper_peer(
    stream: &mut UnixStream,
    expected_gid: u32,
) -> Result<Option<(PeerCred, u32)>> {
    let peer = match get_peer_cred(stream) {
        Ok(peer) => peer,
        Err(reject) => {
            // A cross-check failure is the signature of PID reuse and must
            // be at least as visible as an egid mismatch — emit a structured
            // audit event, not just the warn! already logged in peer_auth.
            match reject {
                PeerAuthReject::CredMismatch {
                    uid,
                    gid,
                    auth_path,
                } => {
                    warn!(?reject, "wrapper peer credential cross-check failed");
                    audit::auth_denied(
                        "wrapper",
                        &format!("peer-cred-mismatch (auth_path {auth_path:?})"),
                        Some(uid),
                        Some(gid),
                    );
                }
                PeerAuthReject::Unresolved => {
                    warn!(?reject, "wrapper peer credentials unresolved");
                    audit::auth_denied("wrapper", "peer-unresolved", None, None);
                }
            }
            write_frame_async(
                stream,
                &WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: PEER_AUTH_WIRE_ERROR.to_string(),
                },
            )
            .await?;
            stream.shutdown().await?;
            return Ok(None);
        }
    };
    let peer_uid = peer.uid;

    // Acquiring the instance setgid group is the sole kernel-enforced
    // gate. The wrapper name carried in the request frame selects which
    // wrapper config to use but grants no privilege on its own.
    if peer.egid != Some(expected_gid) {
        let reason = format!(
            "peer egid {:?} does not match instance group gid {} (auth_path {:?})",
            peer.egid, expected_gid, peer.auth_path,
        );
        warn!(peer = ?peer, "wrapper identity rejected");
        audit::auth_denied("wrapper", &reason, Some(peer_uid), Some(peer.gid));
        write_frame_async(
            stream,
            &WrapperResponse::Error {
                code: ErrorCode::Unauthorized,
                message: PEER_AUTH_WIRE_ERROR.to_string(),
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
    peer: &PeerCred,
    peer_uid: u32,
) -> Result<Option<WrapperRequest>> {
    match timeout(WRAPPER_FIRST_FRAME_TIMEOUT, read_frame_async(stream)).await {
        Ok(Ok(request)) => Ok(Some(request)),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            warn!(peer = ?peer, "wrapper socket first-frame timeout");
            audit::auth_denied(
                "wrapper",
                "timed out waiting for first wrapper request frame",
                Some(peer_uid),
                Some(peer.gid),
            );
            stream.shutdown().await?;
            Ok(None)
        }
    }
}
