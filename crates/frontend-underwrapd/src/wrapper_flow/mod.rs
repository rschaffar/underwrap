use anyhow::Result;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tracing::{info, warn};

use underwrap_protocol::ipc::write_frame_async;
use underwrap_protocol::message::{ErrorCode, WrapperRequest, WrapperResponse};

use crate::peer_auth::PeerCred;
use crate::server::{EnrollmentError, State};

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

    if let Some(enrolled) = result.enrolled.as_ref() {
        establish_scoped_channels(
            &stream,
            &result.response,
            wrapper_name,
            &state,
            enrolled,
            result.resource_access.as_ref(),
        )
        .await;
    }

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

    let enrolled = match state.ensure_enrolled() {
        Ok(e) => e,
        Err(err) => {
            // Log the diagnostic at warn level once per failing request.
            // The user-facing message stays the simple enrollment URL
            // pointer; the diagnostic lives in the journal.
            match &err {
                EnrollmentError::MissingCredential => warn!(
                    wrapper = %wrapper_name,
                    "wrapper request denied: client credential is not on disk yet \
                     (run `underwrap-keygen` for this instance)",
                ),
                EnrollmentError::Backend(e) => warn!(
                    wrapper = %wrapper_name,
                    err = %format!("{e:#}"),
                    "wrapper request denied: backend rejected this client's credential or is unreachable",
                ),
                EnrollmentError::RegisterAssets(e) => warn!(
                    wrapper = %wrapper_name,
                    err = %format!("{e:#}"),
                    "wrapper request denied: backend refused wrapper-asset register",
                ),
            }
            return Ok(read::ReadResult {
                response: WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: crate::server::unenrolled_message(&state.enrollment_url()),
                },
                resource_access: None,
                enrolled: None,
            });
        }
    };

    let approval_satisfied = match check_wrapper_mode(wrapper_name, state, &enrolled) {
        Ok(ModeGate::Proceed) => false,
        Ok(ModeGate::NeedApproval(approval_key)) => {
            match run_approval_round(stream, wrapper_name, &approval_key, peer_uid, state).await? {
                ApprovalOutcome::Approved => true,
                ApprovalOutcome::Rejected(response) => {
                    return Ok(read::ReadResult {
                        response,
                        resource_access: None,
                        enrolled: None,
                    });
                }
            }
        }
        Ok(ModeGate::Deny(response)) => {
            return Ok(read::ReadResult {
                response,
                resource_access: None,
                enrolled: None,
            });
        }
        Err(response) => {
            return Ok(read::ReadResult {
                response,
                resource_access: None,
                enrolled: None,
            });
        }
    };

    let mut result = process_read_for(
        wrapper_name,
        scope,
        state,
        &enrolled,
        peer_uid,
        approval_satisfied,
    );
    // The scoped-channel setup needs the same `Enrolled` we used for
    // the gate check. Hand the snapshot back to the caller.
    result.enrolled = Some(enrolled);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::State;
    use underwrap_protocol::config::{BackendServiceConfig, DaemonConfig};

    /// When the client credential isn't on disk yet (the
    /// just-installed, never-keygen'd case) the daemon must surface
    /// the enrollment URL pointing at the SPA's `/client-instances`
    /// route, and it must do this lazily on the wrapper request rather
    /// than refusing to start.
    #[tokio::test]
    async fn complete_read_returns_enrollment_guidance_when_credential_is_missing() {
        let (mut stream, _peer) = UnixStream::pair().unwrap();
        let state = State::new(DaemonConfig {
            backend: BackendServiceConfig {
                url: "https://backend.example.internal".into(),
                ca_file: Some(std::path::PathBuf::from("/etc/underwrap/ca.pem")),
                client_cert_file: None,
                client_key_file: None,
            },
            wrappers: std::collections::HashMap::new(),
        });

        let result = complete_read(&mut stream, None, "gh-authorized", None, 1000, &state)
            .await
            .unwrap();
        match result.response {
            WrapperResponse::Error { code, message } => {
                assert!(matches!(code, ErrorCode::Unauthorized));
                assert!(message.contains("not enrolled yet"));
                assert!(message.contains("/client-instances"));
            }
            other => panic!("expected enrollment error, got {other:?}"),
        }
        // No `Enrolled` snapshot is handed back to the channel-setup
        // path when the wrapper request short-circuits on enrollment.
        assert!(result.enrolled.is_none());
    }
}
