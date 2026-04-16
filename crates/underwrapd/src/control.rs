use std::sync::Arc;

use anyhow::Result;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tracing::{error, info};

use underwrap_protocol::config::CredentialMode;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{ControlRequest, ControlResponse, ErrorCode};

use crate::audit;
use crate::peer_auth::{check_peer_auth, get_peer_cred};
use crate::policy::backend_policy_active;
use crate::server::State;
use crate::startup::resolve_user_uid;
use crate::status::{build_credential_status, build_wrapper_status};

pub(crate) async fn handle_control(mut stream: UnixStream, state: Arc<State>) -> Result<()> {
    let peer = get_peer_cred(&stream);

    if let Err(reason) = check_peer_auth(
        &peer,
        &state.config.sockets.control_allow_uids,
        &state.config.sockets.control_allow_gids,
    ) {
        audit::auth_denied(
            "control",
            &reason,
            peer.as_ref().map(|p| p.uid),
            peer.as_ref().map(|p| p.gid),
        );
        let response = ControlResponse::Error {
            code: ErrorCode::Unauthorized,
            message: reason,
        };
        write_frame_async(&mut stream, &response).await?;
        stream.shutdown().await?;
        return Ok(());
    }

    let request: ControlRequest = read_frame_async(&mut stream).await?;
    log_control_request(&request, peer.as_ref());

    let peer_uid = peer.as_ref().map(|p| p.uid);
    let response = process_control(&request, &state, peer_uid);

    write_frame_async(&mut stream, &response).await?;
    stream.shutdown().await?;
    Ok(())
}

fn log_control_request(request: &ControlRequest, peer: Option<&crate::peer_auth::PeerCred>) {
    match request {
        ControlRequest::StatusAll => {
            info!(command = "status_all", peer = ?peer, "control request");
        }
        ControlRequest::StatusTarget { target } => {
            info!(command = "status_target", target = %target, peer = ?peer, "control request");
        }
        ControlRequest::Store { secret_id, .. } => {
            info!(command = "store", secret_id = %secret_id, peer = ?peer, "control request");
        }
        ControlRequest::Unlock {
            target,
            scope,
            seconds,
            ..
        } => {
            info!(command = "unlock", target = %target, scope = ?scope, seconds = ?seconds, peer = ?peer, "control request");
        }
        ControlRequest::Lock { target, scope } => {
            info!(command = "lock", target = ?target, scope = ?scope, peer = ?peer, "control request");
        }
    }
}

pub(crate) fn process_control(
    request: &ControlRequest,
    state: &State,
    peer_uid: Option<u32>,
) -> ControlResponse {
    match request {
        ControlRequest::StatusAll => build_status_all_response(state),
        ControlRequest::StatusTarget { target } => build_status_target_response(target, state),
        ControlRequest::Unlock {
            target,
            scope,
            seconds,
            grant_user,
            grant_all,
        } => process_unlock_request(
            target,
            scope.as_deref(),
            *seconds,
            grant_user,
            *grant_all,
            state,
            peer_uid,
        ),
        ControlRequest::Lock { target, scope } => match backend_policy_active(&*state.store) {
            Ok(true) => ControlResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "backend-managed policy is active; local lock is disabled".to_string(),
            },
            Ok(false) => {
                state.unlock.lock(target.as_deref(), scope.as_deref());
                audit::locked(target.as_deref(), scope.as_deref(), peer_uid);
                ControlResponse::LockOk
            }
            Err(_) => ControlResponse::Error {
                code: ErrorCode::Internal,
                message: "internal policy error".to_string(),
            },
        },
        ControlRequest::Store {
            secret_id,
            material,
        } => process_store_request(secret_id, material.as_ref(), state, peer_uid),
    }
}

fn build_status_all_response(state: &State) -> ControlResponse {
    let mut wrappers: Vec<String> = state.config.wrappers.keys().cloned().collect();
    wrappers.sort();
    let mut credentials: Vec<String> = state.config.credentials.keys().cloned().collect();
    credentials.sort();
    ControlResponse::StatusAllOk {
        wrappers,
        credentials,
    }
}

fn build_status_target_response(target: &str, state: &State) -> ControlResponse {
    if let Some(status) = build_wrapper_status(target, &state.config, &state.unlock, &*state.store)
    {
        ControlResponse::StatusWrapperOk { status }
    } else if let Some(status) = build_credential_status(target, &state.config, &*state.store) {
        ControlResponse::StatusCredentialOk { status }
    } else {
        ControlResponse::Error {
            code: ErrorCode::NotFound,
            message: format!("unknown target '{target}'"),
        }
    }
}

fn process_unlock_request(
    target: &str,
    scope: Option<&str>,
    seconds: Option<u64>,
    grant_user: &Option<String>,
    grant_all: bool,
    state: &State,
    peer_uid: Option<u32>,
) -> ControlResponse {
    let wrapper = match state.config.wrappers.get(target) {
        Some(w) => w,
        None => {
            return ControlResponse::Error {
                code: ErrorCode::NotFound,
                message: format!("unknown wrapper '{target}'"),
            };
        }
    };

    match backend_policy_active(&*state.store) {
        Ok(true) => {
            return ControlResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "backend-managed policy is active; local unlock is disabled".to_string(),
            };
        }
        Ok(false) => {}
        Err(_) => {
            return ControlResponse::Error {
                code: ErrorCode::Internal,
                message: "internal policy error".to_string(),
            };
        }
    }

    if let Some(requested) = seconds
        && let Some(max) = wrapper.unlock.max_seconds
        && requested > max
    {
        return ControlResponse::Error {
            code: ErrorCode::Unauthorized,
            message: format!("requested unlock duration {requested}s exceeds maxSeconds {max}s"),
        };
    }

    let caller_uid = match peer_uid {
        Some(uid) => uid,
        None => {
            return ControlResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "could not determine caller uid".to_string(),
            };
        }
    };

    let grant_uids = match resolve_unlock_grantees(caller_uid, grant_user, grant_all, state) {
        Ok(uids) => uids,
        Err(err) => return err.into_control_response(),
    };

    for uid in grant_uids {
        state.unlock.unlock(target, scope, seconds, uid);
    }
    audit::unlocked(target, scope, seconds, Some(caller_uid));
    ControlResponse::UnlockOk
}

enum UnlockGrantError {
    UnknownUser(String),
    UserNotAllowed(String),
    RootMustSpecifyTarget,
    NonRootMayNotGrant,
}

impl UnlockGrantError {
    fn into_control_response(self) -> ControlResponse {
        match self {
            Self::UnknownUser(message) => ControlResponse::Error {
                code: ErrorCode::NotFound,
                message,
            },
            Self::UserNotAllowed(message) => ControlResponse::Error {
                code: ErrorCode::Unauthorized,
                message,
            },
            Self::RootMustSpecifyTarget => ControlResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "root must specify --for-user or --for-all".to_string(),
            },
            Self::NonRootMayNotGrant => ControlResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "only root may use --for-user or --for-all".to_string(),
            },
        }
    }
}

fn resolve_unlock_grantees(
    caller_uid: u32,
    grant_user: &Option<String>,
    grant_all: bool,
    state: &State,
) -> std::result::Result<Vec<u32>, UnlockGrantError> {
    if caller_uid == 0 {
        if grant_all {
            return Ok(state.allowed_user_uids.iter().copied().collect());
        }

        if let Some(user) = grant_user {
            let uid = match resolve_user_uid(user) {
                Ok(uid) => uid,
                Err(e) => {
                    return Err(UnlockGrantError::UnknownUser(format!(
                        "unknown user '{user}': {e}"
                    )));
                }
            };
            if !state.allowed_user_uids.is_empty() && !state.allowed_user_uids.contains(&uid) {
                return Err(UnlockGrantError::UserNotAllowed(format!(
                    "user '{user}' is not allowed for this instance"
                )));
            }
            return Ok(vec![uid]);
        }

        return Err(UnlockGrantError::RootMustSpecifyTarget);
    }

    if grant_user.is_some() || grant_all {
        return Err(UnlockGrantError::NonRootMayNotGrant);
    }

    Ok(vec![caller_uid])
}

fn process_store_request(
    secret_id: &str,
    material: &[u8],
    state: &State,
    peer_uid: Option<u32>,
) -> ControlResponse {
    let Some(credential) = state.config.credentials.get(secret_id) else {
        return ControlResponse::Error {
            code: ErrorCode::NotFound,
            message: format!("credential '{secret_id}' not declared in configuration"),
        };
    };
    if !matches!(credential.mode, CredentialMode::Env { .. }) {
        return ControlResponse::Error {
            code: ErrorCode::Unauthorized,
            message: format!(
                "credential '{secret_id}' is not an env credential; use the wrapped tool or a mode-specific import path instead"
            ),
        };
    }

    match state.store.write(secret_id, material) {
        Ok(()) => {
            audit::secret_stored(secret_id, peer_uid);
            ControlResponse::StoreOk
        }
        Err(e) => {
            error!(secret = %secret_id, err = %e, "storage write error");
            ControlResponse::Error {
                code: ErrorCode::Internal,
                message: "internal storage error".to_string(),
            }
        }
    }
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
