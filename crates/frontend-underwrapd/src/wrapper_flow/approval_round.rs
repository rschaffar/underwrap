use anyhow::Result;
use tokio::net::UnixStream;
use tokio::time::{Duration, timeout};
use tracing::{error, warn};

use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{ErrorCode, WrapperRequest, WrapperResponse};
use underwrap_protocol::policy::PolicyDecision;

use crate::approval::{self, IssuedChallenge};
use crate::audit;
use crate::policy::authorize_grant;
use crate::server::State;
use crate::startup::username_for_uid;

const WRAPPER_APPROVAL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) enum ModeGate {
    Proceed,
    NeedApproval(String),
    Deny(WrapperResponse),
}

/// Consult the per-store policy document for the `(wrapper, host, user)`
/// grant tied to `peer_uid`. Returns:
///
/// - `Proceed`       — grant is `Unlocked` or `UnlockedUntil` and within
///   its deadline; the read flow runs without an approval round.
/// - `NeedApproval`  — grant is `PerCallApproval`; caller runs the
///   approval round before proceeding.
/// - `Deny`          — no grant, or grant is `Locked` / past its
///   `UnlockedUntil` deadline. The error response is pre-baked so the
///   caller can ship it back to the wrapper without further work.
pub(super) fn check_wrapper_mode(
    wrapper_name: &str,
    peer_uid: u32,
    state: &State,
) -> Result<ModeGate, WrapperResponse> {
    let Some(user) = username_for_uid(peer_uid) else {
        return Ok(ModeGate::Deny(WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: format!("no passwd entry for peer uid {peer_uid}"),
        }));
    };
    let host = state.config.catalog_host.as_str();
    let wrapper_def = state.config.wrappers.get(wrapper_name);
    let logical_name = wrapper_def
        .and_then(|w| w.logical_name.clone())
        .unwrap_or_else(|| wrapper_name.to_string());
    match authorize_grant(
        state
            .store
            .as_ref()
            .expect("approval checks only run for enrolled instances")
            .as_ref(),
        &logical_name,
        host,
        &user,
    ) {
        Ok(Some(PolicyDecision::Allow)) => Ok(ModeGate::Proceed),
        Ok(Some(PolicyDecision::NeedsApproval { approval_key })) => {
            Ok(ModeGate::NeedApproval(approval_key))
        }
        Ok(Some(PolicyDecision::Deny { reason })) => Ok(ModeGate::Deny(WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: reason,
        })),
        Ok(None) => Ok(ModeGate::Deny(WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: format!("no grant for wrapper '{logical_name}' on {host} as user '{user}'"),
        })),
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
