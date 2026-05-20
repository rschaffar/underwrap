use anyhow::Result;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tracing::info;

use underwrap_protocol::ipc::write_frame_async;
use underwrap_protocol::message::{ErrorCode, WrapperRequest, WrapperResponse};

use crate::peer_auth::PeerCred;
use crate::server::State;

mod approval_round;
mod channels;
mod intake;
mod read;

use approval_round::{ApprovalOutcome, ModeGate, check_wrapper_mode, run_approval_round};
use channels::establish_scoped_channels;
use intake::{authorize_wrapper_peer, read_initial_wrapper_request};
use read::process_read_for;

pub(crate) async fn handle_wrapper(
    mut stream: UnixStream,
    state: Arc<State>,
    wrapper_name: &str,
    expected_gid: u32,
) -> Result<()> {
    let (peer, peer_uid) =
        match authorize_wrapper_peer(&mut stream, &state, wrapper_name, expected_gid).await? {
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
        WrapperRequest::ApprovalProof { .. } => {
            write_frame_async(
                &mut stream,
                &WrapperResponse::Error {
                    code: ErrorCode::Internal,
                    message: "first frame must be Read".to_string(),
                },
            )
            .await?;
            stream.shutdown().await?;
            return Ok(());
        }
    };

    let result = complete_read(
        &mut stream,
        scope.as_deref(),
        wrapper_name,
        peer.as_ref(),
        peer_uid,
        &state,
    )
    .await?;
    write_frame_async(&mut stream, &result.response).await?;

    establish_scoped_channels(
        &stream,
        &result.response,
        wrapper_name,
        &state,
        result.resource_access.as_ref(),
    )
    .await;

    stream.shutdown().await?;
    Ok(())
}

/// Run the full read flow, including a per-call approval round-trip when
/// backend policy demands one.
async fn complete_read(
    stream: &mut UnixStream,
    scope: Option<&str>,
    wrapper_name: &str,
    peer: Option<&PeerCred>,
    peer_uid: u32,
    state: &State,
) -> Result<read::ReadResult> {
    info!(wrapper = %wrapper_name, peer = ?peer, "wrapper read request");

    if let Some(url) = state.enrollment_url.as_deref() {
        return Ok(read::ReadResult {
            response: WrapperResponse::Error {
                code: ErrorCode::Unauthorized,
                message: crate::server::unenrolled_message(url),
            },
            resource_access: None,
        });
    }

    let approval_satisfied = match check_wrapper_mode(wrapper_name, state) {
        Ok(ModeGate::Proceed) => false,
        Ok(ModeGate::NeedApproval(approval_key)) => {
            match run_approval_round(stream, wrapper_name, &approval_key, peer_uid, state).await? {
                ApprovalOutcome::Approved => true,
                ApprovalOutcome::Rejected(response) => {
                    return Ok(read::ReadResult {
                        response,
                        resource_access: None,
                    });
                }
            }
        }
        Ok(ModeGate::Deny(response)) => {
            return Ok(read::ReadResult {
                response,
                resource_access: None,
            });
        }
        Err(response) => {
            return Ok(read::ReadResult {
                response,
                resource_access: None,
            });
        }
    };

    Ok(process_read_for(
        wrapper_name,
        scope,
        state,
        peer_uid,
        approval_satisfied,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::PendingApprovals;
    use crate::server::State;
    use underwrap_protocol::config::{BackendServiceConfig, DaemonConfig};

    #[tokio::test]
    async fn complete_read_returns_enrollment_guidance_when_instance_is_unenrolled() {
        let (mut stream, _peer) = UnixStream::pair().unwrap();
        let state = State {
            config: DaemonConfig {
                backend: BackendServiceConfig {
                    url: "https://backend.example.internal".into(),
                    ca_file: Some(std::path::PathBuf::from("/etc/underwrap/ca.pem")),
                    client_cert_file: None,
                    client_key_file: None,
                },
                wrappers: std::collections::HashMap::new(),
            },
            store: None,
            enrollment_url: Some("https://backend.example.internal/enroll".into()),
            client_instance: String::new(),
            pending_approvals: PendingApprovals::new(),
        };

        let result = complete_read(&mut stream, None, "gh-authorized", None, 1000, &state)
            .await
            .unwrap();
        match result.response {
            WrapperResponse::Error { code, message } => {
                assert!(matches!(code, ErrorCode::Unauthorized));
                assert!(message.contains("not enrolled yet"));
                assert!(message.contains("/enroll"));
            }
            other => panic!("expected enrollment error, got {other:?}"),
        }
    }
}
