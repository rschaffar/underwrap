use anyhow::Result;
use tokio::net::UnixStream;
use tokio::time::{Duration, timeout};
use tracing::{error, warn};

use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{ErrorCode, WrapperRequest, WrapperResponse};
use underwrap_protocol::policy::PolicyDecision;

use crate::approval::{self, IssuedChallenge};
use crate::audit;
use crate::policy::authorize_operation;
use crate::server::State;

const WRAPPER_APPROVAL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) enum ModeGate {
    Proceed,
    NeedApproval(String),
}

pub(super) fn check_wrapper_mode(
    wrapper_name: &str,
    state: &State,
) -> Result<ModeGate, WrapperResponse> {
    // Probe the policy document by evaluating a dummy tuple: we only care
    // whether the mode gate fires. We pick a sentinel resource/operation
    // pair; this never allow-matches any real rule.
    match authorize_operation(
        &*state.store,
        wrapper_name,
        "__underwrap/mode-probe",
        "__underwrap.mode-probe",
    ) {
        Ok(PolicyDecision::NeedsApproval { approval_key }) => {
            Ok(ModeGate::NeedApproval(approval_key))
        }
        Ok(_) => Ok(ModeGate::Proceed),
        Err(e) => {
            error!(wrapper = %wrapper_name, err = %e, "policy lookup error");
            Err(WrapperResponse::Error {
                code: ErrorCode::Internal,
                message: "internal policy error".to_string(),
            })
        }
    }
}

pub(super) enum ApprovalOutcome {
    Approved,
    Rejected(WrapperResponse),
}

pub(super) async fn run_approval_round(
    stream: &mut UnixStream,
    wrapper_name: &str,
    approval_key: &str,
    peer_uid: u32,
    state: &State,
) -> Result<ApprovalOutcome> {
    let IssuedChallenge {
        challenge_id,
        nonce,
        approval_key,
    } = state.pending_approvals.issue(wrapper_name, approval_key);
    write_frame_async(
        stream,
        &WrapperResponse::ApprovalRequired {
            challenge_id: challenge_id.clone(),
            nonce,
            approval_key,
        },
    )
    .await?;

    let proof_request =
        match timeout(WRAPPER_APPROVAL_RESPONSE_TIMEOUT, read_frame_async(stream)).await {
            Ok(Ok(req)) => req,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                warn!(wrapper = %wrapper_name, "approval challenge follow-up timeout");
                return Ok(ApprovalOutcome::Rejected(WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: "approval challenge timeout".to_string(),
                }));
            }
        };

    let (proof_challenge_id, signature, public_key_blob) = match proof_request {
        WrapperRequest::ApprovalProof {
            challenge_id,
            signature,
            public_key_blob,
        } => (challenge_id, signature, public_key_blob),
        WrapperRequest::Read { .. } => {
            return Ok(ApprovalOutcome::Rejected(WrapperResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "expected ApprovalProof after ApprovalRequired".to_string(),
            }));
        }
    };

    if proof_challenge_id != challenge_id {
        return Ok(ApprovalOutcome::Rejected(WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: "approval challenge id mismatch".to_string(),
        }));
    }

    let (stored_wrapper, stored_nonce, stored_key) =
        match state.pending_approvals.consume(&proof_challenge_id) {
            Ok(entry) => entry,
            Err(e) => {
                return Ok(ApprovalOutcome::Rejected(WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: format!("approval challenge not found: {e}"),
                }));
            }
        };
    if stored_wrapper != wrapper_name {
        return Ok(ApprovalOutcome::Rejected(WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: "approval challenge wrapper mismatch".to_string(),
        }));
    }

    if let Err(e) = approval::verify_proof(&stored_key, &stored_nonce, &signature, &public_key_blob)
    {
        audit::auth_denied("approval", &e.to_string(), Some(peer_uid), None);
        return Ok(ApprovalOutcome::Rejected(WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: "approval signature rejected".to_string(),
        }));
    }

    audit::approved(wrapper_name, None, None, Some(peer_uid));
    Ok(ApprovalOutcome::Approved)
}
