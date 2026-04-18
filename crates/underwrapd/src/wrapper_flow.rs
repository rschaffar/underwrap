use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::time::{Duration, timeout};
use tracing::{error, info, warn};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{ErrorCode, WrapperRequest, WrapperResponse};
use underwrap_protocol::policy::PolicyDecision;

use crate::approval::{self, IssuedChallenge};
use crate::audit;
use crate::peer_auth::{PeerCred, get_peer_cred};
use crate::policy::{
    authorize_after_approval, authorize_operation, directory_resource, env_resource,
    libsecret_resource,
};
use crate::server::State;
use crate::wrapper_bindings::{
    wrapper_directory_bindings, wrapper_env_credentials, wrapper_libsecret_binding,
};

const WRAPPER_FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(2);
const WRAPPER_APPROVAL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

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

    let response = complete_read(
        &mut stream,
        scope.as_deref(),
        wrapper_name,
        peer.as_ref(),
        peer_uid,
        &state,
    )
    .await?;
    write_frame_async(&mut stream, &response).await?;

    establish_scoped_channels(&stream, &response, wrapper_name, &state).await;

    stream.shutdown().await?;
    Ok(())
}

async fn authorize_wrapper_peer(
    stream: &mut UnixStream,
    state: &State,
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

    if !state.allowed_user_uids.is_empty() && !state.allowed_user_uids.contains(&peer_uid) {
        let reason = format!("peer uid {peer_uid} is not allowed for this instance");
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

async fn read_initial_wrapper_request(
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

/// Run the full read flow, including a per-call approval round-trip when
/// backend policy demands one.
async fn complete_read(
    stream: &mut UnixStream,
    scope: Option<&str>,
    wrapper_name: &str,
    peer: Option<&PeerCred>,
    peer_uid: u32,
    state: &State,
) -> Result<WrapperResponse> {
    info!(wrapper = %wrapper_name, peer = ?peer, "wrapper read request");

    let approval_satisfied = match check_wrapper_mode(wrapper_name, state) {
        Ok(ModeGate::Proceed) => false,
        Ok(ModeGate::NeedApproval(approval_key)) => {
            match run_approval_round(stream, wrapper_name, &approval_key, peer_uid, state).await? {
                ApprovalOutcome::Approved => true,
                ApprovalOutcome::Rejected(response) => return Ok(response),
            }
        }
        Err(response) => return Ok(response),
    };

    Ok(process_read_for(wrapper_name, scope, state, peer_uid, approval_satisfied).await)
}

enum ModeGate {
    Proceed,
    NeedApproval(String),
}

fn check_wrapper_mode(wrapper_name: &str, state: &State) -> Result<ModeGate, WrapperResponse> {
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

enum ApprovalOutcome {
    Approved,
    Rejected(WrapperResponse),
}

async fn run_approval_round(
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

async fn establish_scoped_channels(
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

/// Run the post-gate read: rule/grant authorization for each credential
/// and fetch of any env-bound secret material. When `approval_satisfied`
/// is true, mode gating is bypassed (the caller just proved presence).
pub(crate) async fn process_read_for(
    wrapper_name: &str,
    scope: Option<&str>,
    state: &State,
    peer_uid: u32,
    approval_satisfied: bool,
) -> WrapperResponse {
    let wrapper_def = match state.config.wrappers.get(wrapper_name) {
        Some(w) => w,
        None => {
            warn!(wrapper = %wrapper_name, "unknown wrapper");
            return WrapperResponse::Error {
                code: ErrorCode::NotFound,
                message: format!("unknown wrapper '{wrapper_name}'"),
            };
        }
    };

    let authorize = |resource: &str, operation: &str| {
        if approval_satisfied {
            authorize_after_approval(&*state.store, wrapper_name, resource, operation)
        } else {
            authorize_operation(&*state.store, wrapper_name, resource, operation)
        }
    };

    if let Some((credential_ref, access)) =
        wrapper_libsecret_binding(wrapper_def, &state.config.credentials)
    {
        let resource = libsecret_resource(credential_ref);
        let operation = match access {
            AccessMode::ReadOnly => "libsecret.read",
            AccessMode::ReadWrite => "libsecret.readwrite",
        };
        match authorize(&resource, operation) {
            Ok(PolicyDecision::Allow) => {
                audit::read_granted(wrapper_name, credential_ref, Some(peer_uid), scope);
            }
            Ok(PolicyDecision::Deny { reason }) => {
                audit::read_denied(wrapper_name, &reason, Some(peer_uid), scope);
                return WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: reason,
                };
            }
            Ok(PolicyDecision::NeedsApproval { .. }) => {
                let reason = format!("wrapper '{wrapper_name}' requires per-call approval");
                audit::read_denied(wrapper_name, &reason, Some(peer_uid), scope);
                return WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: reason,
                };
            }
            Err(e) => {
                error!(credential = %credential_ref, err = %e, "policy lookup error");
                return WrapperResponse::Error {
                    code: ErrorCode::Internal,
                    message: "internal policy error".to_string(),
                };
            }
        }
    }

    let mut env = std::collections::HashMap::new();
    for (cred_ref, variable) in wrapper_env_credentials(wrapper_def, &state.config.credentials) {
        let resource = env_resource(cred_ref);
        match authorize(&resource, "secret.read") {
            Ok(PolicyDecision::Allow) => {}
            Ok(PolicyDecision::Deny { reason }) => {
                audit::read_denied(wrapper_name, &reason, Some(peer_uid), scope);
                return WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: reason,
                };
            }
            Ok(PolicyDecision::NeedsApproval { .. }) => {
                let reason = format!("wrapper '{wrapper_name}' requires per-call approval");
                audit::read_denied(wrapper_name, &reason, Some(peer_uid), scope);
                return WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: reason,
                };
            }
            Err(e) => {
                error!(credential = %cred_ref, err = %e, "policy lookup error");
                return WrapperResponse::Error {
                    code: ErrorCode::Internal,
                    message: "internal policy error".to_string(),
                };
            }
        }
        match state.store.read(cred_ref) {
            Ok(Some(material)) => {
                audit::read_granted(wrapper_name, cred_ref, Some(peer_uid), scope);
                env.insert(variable.to_string(), material);
            }
            Ok(None) => {
                let msg = format!("credential '{cred_ref}' not found in store");
                audit::read_denied(wrapper_name, &msg, Some(peer_uid), scope);
                return WrapperResponse::Error {
                    code: ErrorCode::NotFound,
                    message: msg,
                };
            }
            Err(e) => {
                error!(credential = %cred_ref, err = %e, "storage read error");
                return WrapperResponse::Error {
                    code: ErrorCode::Internal,
                    message: "internal storage error".to_string(),
                };
            }
        }
    }

    let mut directories = Vec::new();
    for binding in wrapper_directory_bindings(wrapper_def, &state.config.credentials) {
        let resource = directory_resource(&binding.credential_ref);
        let operation = match binding.access {
            AccessMode::ReadOnly => "directory.read",
            AccessMode::ReadWrite => "directory.readwrite",
        };
        match authorize(&resource, operation) {
            Ok(PolicyDecision::Allow) => {
                audit::read_granted(wrapper_name, &binding.credential_ref, Some(peer_uid), scope);
                directories.push(binding);
            }
            Ok(PolicyDecision::Deny { reason }) => {
                audit::read_denied(wrapper_name, &reason, Some(peer_uid), scope);
                return WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: reason,
                };
            }
            Ok(PolicyDecision::NeedsApproval { .. }) => {
                let reason = format!("wrapper '{wrapper_name}' requires per-call approval");
                audit::read_denied(wrapper_name, &reason, Some(peer_uid), scope);
                return WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: reason,
                };
            }
            Err(e) => {
                error!(credential = %binding.credential_ref, err = %e, "policy lookup error");
                return WrapperResponse::Error {
                    code: ErrorCode::Internal,
                    message: "internal policy error".to_string(),
                };
            }
        }
    }

    WrapperResponse::ReadOk { env, directories }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use underwrap_protocol::config::{
        AccessMode, BackendSocketConfig, CredentialDef, CredentialMode, CredentialUse,
        DaemonConfig, SocketConfig, WrapperDef,
    };
    use underwrap_protocol::message::SecretBytes;

    struct MemStore {
        data: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MemStore {
        fn with_entries(entries: &[(&str, &[u8])]) -> Self {
            Self {
                data: Mutex::new(
                    entries
                        .iter()
                        .map(|(k, v)| ((*k).to_string(), v.to_vec()))
                        .collect(),
                ),
            }
        }

        fn with_policy(
            entries: &[(&str, &[u8])],
            document: underwrap_protocol::policy::PolicyDocument,
        ) -> Self {
            let mut data: HashMap<String, Vec<u8>> = entries
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.to_vec()))
                .collect();
            data.insert(
                underwrap_protocol::policy::POLICY_DOCUMENT_SECRET_ID.to_string(),
                serde_json::to_vec(&document).unwrap(),
            );
            Self {
                data: Mutex::new(data),
            }
        }
    }

    impl underwrap_store::StorageBackend for MemStore {
        fn read(&self, secret_id: &str) -> anyhow::Result<Option<SecretBytes>> {
            Ok(self
                .data
                .lock()
                .unwrap()
                .get(secret_id)
                .cloned()
                .map(Into::into))
        }
        fn write(&self, secret_id: &str, material: &[u8]) -> anyhow::Result<()> {
            self.data
                .lock()
                .unwrap()
                .insert(secret_id.to_string(), material.to_vec());
            Ok(())
        }
        fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
            let mut ids: Vec<_> = self
                .data
                .lock()
                .unwrap()
                .keys()
                .filter(|id| id.starts_with(prefix))
                .cloned()
                .collect();
            ids.sort();
            Ok(ids)
        }
        fn delete(&self, secret_id: &str) -> anyhow::Result<bool> {
            Ok(self.data.lock().unwrap().remove(secret_id).is_some())
        }
    }

    fn state_with_store(store: Arc<dyn underwrap_store::StorageBackend>) -> State {
        State {
            config: DaemonConfig {
                sockets: SocketConfig::default(),
                backend: BackendSocketConfig {
                    socket: PathBuf::from("/tmp/backend.sock"),
                    ..Default::default()
                },
                allowed_users: Vec::new(),
                credentials: HashMap::from([
                    (
                        "github/token".to_string(),
                        CredentialDef {
                            mode: CredentialMode::Env {
                                variable: "GH_TOKEN".to_string(),
                            },
                        },
                    ),
                    (
                        "gws/state".to_string(),
                        CredentialDef {
                            mode: CredentialMode::Directory {
                                path: ".config/gcloud".to_string(),
                            },
                        },
                    ),
                ]),
                wrappers: HashMap::from([(
                    "gh-authorized".to_string(),
                    WrapperDef {
                        target: PathBuf::from("/usr/bin/gh"),
                        use_map: HashMap::from([
                            (
                                "github/token".to_string(),
                                CredentialUse {
                                    access: AccessMode::ReadOnly,
                                },
                            ),
                            (
                                "gws/state".to_string(),
                                CredentialUse {
                                    access: AccessMode::ReadWrite,
                                },
                            ),
                        ]),
                        socket: PathBuf::from("/tmp/test.sock"),
                        group: "test-group".to_string(),
                    },
                )]),
            },
            store,
            allowed_user_uids: std::collections::HashSet::new(),
            pending_approvals: approval::PendingApprovals::new(),
        }
    }

    #[tokio::test]
    async fn process_read_for_returns_env_and_directory_bindings() {
        let state = state_with_store(Arc::new(MemStore::with_entries(&[(
            "github/token",
            b"ghp_test",
        )])));
        let resp = process_read_for("gh-authorized", None, &state, 1000, true).await;
        match resp {
            WrapperResponse::ReadOk { env, directories } => {
                assert_eq!(env.get("GH_TOKEN").unwrap(), &SecretBytes::from("ghp_test"));
                assert_eq!(directories.len(), 1);
                assert_eq!(directories[0].credential_ref, "gws/state");
            }
            other => panic!("expected ReadOk, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn process_read_for_denies_when_backend_policy_lacks_required_rule() {
        let store: Arc<dyn underwrap_store::StorageBackend> = Arc::new(MemStore::with_policy(
            &[("github/token", b"ghp_test")],
            underwrap_protocol::policy::PolicyDocument::default(),
        ));
        let state = state_with_store(store);
        let resp = process_read_for("gh-authorized", None, &state, 1000, false).await;
        match resp {
            WrapperResponse::Error { code, message } => {
                assert!(matches!(code, ErrorCode::Unauthorized));
                assert!(message.contains("no backend policy rule"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn process_read_for_allows_when_backend_policy_has_required_rules() {
        let store: Arc<dyn underwrap_store::StorageBackend> = Arc::new(MemStore::with_policy(
            &[("github/token", b"ghp_test")],
            underwrap_protocol::policy::PolicyDocument {
                wrappers: Vec::new(),
                rules: vec![
                    underwrap_protocol::policy::PolicyRule {
                        id: "env-read".to_string(),
                        wrapper: "gh-authorized".to_string(),
                        resource: "secret:github/token".to_string(),
                        operation: "secret.read".to_string(),
                        effect: underwrap_protocol::policy::PolicyEffect::Allow,
                    },
                    underwrap_protocol::policy::PolicyRule {
                        id: "dir-rw".to_string(),
                        wrapper: "gh-authorized".to_string(),
                        resource: "directory:gws/state".to_string(),
                        operation: "directory.readwrite".to_string(),
                        effect: underwrap_protocol::policy::PolicyEffect::Allow,
                    },
                ],
                grants: Vec::new(),
            },
        ));
        let state = state_with_store(store);
        let resp = process_read_for("gh-authorized", None, &state, 1000, false).await;
        match resp {
            WrapperResponse::ReadOk { env, directories } => {
                assert_eq!(env.get("GH_TOKEN").unwrap(), &SecretBytes::from("ghp_test"));
                assert_eq!(directories.len(), 1);
            }
            other => panic!("expected ReadOk, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn process_read_for_requires_per_call_approval_when_mode_demands() {
        let store: Arc<dyn underwrap_store::StorageBackend> = Arc::new(MemStore::with_policy(
            &[("github/token", b"ghp_test")],
            underwrap_protocol::policy::PolicyDocument {
                wrappers: vec![underwrap_protocol::policy::WrapperPolicyState {
                    wrapper: "gh-authorized".to_string(),
                    mode: underwrap_protocol::policy::WrapperApprovalMode::PerCallApproval {
                        approval_key: "ssh-ed25519 AAAA".to_string(),
                    },
                    reason: None,
                }],
                rules: vec![
                    underwrap_protocol::policy::PolicyRule {
                        id: "env-read".to_string(),
                        wrapper: "gh-authorized".to_string(),
                        resource: "secret:github/token".to_string(),
                        operation: "secret.read".to_string(),
                        effect: underwrap_protocol::policy::PolicyEffect::Allow,
                    },
                    underwrap_protocol::policy::PolicyRule {
                        id: "dir-rw".to_string(),
                        wrapper: "gh-authorized".to_string(),
                        resource: "directory:gws/state".to_string(),
                        operation: "directory.readwrite".to_string(),
                        effect: underwrap_protocol::policy::PolicyEffect::Allow,
                    },
                ],
                grants: Vec::new(),
            },
        ));
        let state = state_with_store(store);

        // Without the approval marker, authorization bubbles the NeedsApproval
        // shape back as an Unauthorized error.
        let resp = process_read_for("gh-authorized", None, &state, 1000, false).await;
        assert!(matches!(resp, WrapperResponse::Error { .. }));

        // With approval satisfied, the same request succeeds.
        let resp = process_read_for("gh-authorized", None, &state, 1000, true).await;
        assert!(matches!(resp, WrapperResponse::ReadOk { .. }));
    }
}
