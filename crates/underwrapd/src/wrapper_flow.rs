use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::time::{Duration, timeout};
use tracing::{error, info, warn};

use underwrap_protocol::config::{AccessMode, UnlockMode};
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{ErrorCode, WrapperRequest, WrapperResponse};

use crate::audit;
use crate::peer_auth::{PeerCred, get_peer_cred};
use crate::policy::{
    authorize_operation, backend_policy_active, directory_resource, env_resource,
    libsecret_resource,
};
use crate::server::State;
use crate::unlock_flow::{process_wrapper_unlock, process_wrapper_unlock_response};
use crate::wrapper_bindings::{
    wrapper_directory_bindings, wrapper_env_credentials, wrapper_libsecret_binding,
};

const WRAPPER_FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(2);
const WRAPPER_UNLOCK_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

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

    let response =
        dispatch_wrapper_request(&request, wrapper_name, peer.as_ref(), peer_uid, &state).await;
    write_frame_async(&mut stream, &response).await?;

    if let Some(follow_up_response) = handle_unlock_follow_up(
        &mut stream,
        &response,
        wrapper_name,
        peer.as_ref(),
        peer_uid,
        &state,
    )
    .await?
    {
        write_frame_async(&mut stream, &follow_up_response).await?;
    }

    establish_scoped_channels(&stream, &request, &response, wrapper_name, &state).await;

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

async fn dispatch_wrapper_request(
    request: &WrapperRequest,
    wrapper_name: &str,
    peer: Option<&PeerCred>,
    peer_uid: u32,
    state: &State,
) -> WrapperResponse {
    match request {
        WrapperRequest::Read { scope } => {
            info!(wrapper = %wrapper_name, peer = ?peer, "wrapper read request");
            process_read_for(wrapper_name, scope.as_deref(), state, peer_uid).await
        }
        WrapperRequest::Unlock { scope, seconds } => {
            info!(wrapper = %wrapper_name, peer = ?peer, "unlock request via wrapper socket");
            process_wrapper_unlock(wrapper_name, scope.as_deref(), *seconds, peer_uid, state)
        }
        WrapperRequest::UnlockResponse {
            scope,
            seconds,
            challenge,
            signature,
            public_key_blob,
        } => {
            info!(wrapper = %wrapper_name, peer = ?peer, "unlock challenge response");
            process_wrapper_unlock_response(
                wrapper_name,
                scope.as_deref(),
                *seconds,
                peer_uid,
                state,
                challenge,
                signature,
                public_key_blob,
            )
        }
    }
}

async fn handle_unlock_follow_up(
    stream: &mut UnixStream,
    response: &WrapperResponse,
    wrapper_name: &str,
    peer: Option<&PeerCred>,
    peer_uid: u32,
    state: &State,
) -> Result<Option<WrapperResponse>> {
    if !matches!(response, WrapperResponse::Challenge { .. }) {
        return Ok(None);
    }

    let follow_up = match timeout(WRAPPER_UNLOCK_RESPONSE_TIMEOUT, read_frame_async(stream)).await {
        Ok(Ok(request)) => request,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            warn!(wrapper = %wrapper_name, peer = ?peer, "unlock challenge follow-up timeout");
            stream.shutdown().await?;
            return Ok(None);
        }
    };

    let follow_up_response = match &follow_up {
        WrapperRequest::UnlockResponse {
            scope,
            seconds,
            challenge,
            signature,
            public_key_blob,
        } => {
            info!(wrapper = %wrapper_name, peer = ?peer, "unlock challenge response (same connection)");
            process_wrapper_unlock_response(
                wrapper_name,
                scope.as_deref(),
                *seconds,
                peer_uid,
                state,
                challenge,
                signature,
                public_key_blob,
            )
        }
        _ => WrapperResponse::Error {
            code: ErrorCode::Internal,
            message: "expected UnlockResponse after Challenge".to_string(),
        },
    };

    Ok(Some(follow_up_response))
}

async fn establish_scoped_channels(
    stream: &UnixStream,
    request: &WrapperRequest,
    response: &WrapperResponse,
    wrapper_name: &str,
    state: &State,
) {
    if !matches!(response, WrapperResponse::ReadOk { .. })
        || !matches!(request, WrapperRequest::Read { .. })
    {
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
        match authorize_operation(&*state.store, wrapper_name, &resource, operation) {
            Ok(underwrap_protocol::policy::PolicyDecision::Allow) => {}
            Ok(underwrap_protocol::policy::PolicyDecision::Deny { reason }) => {
                error!(wrapper = %wrapper_name, credential = %credential_ref, reason = %reason, "backend policy denied libsecret channel");
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
        match authorize_operation(&*state.store, wrapper_name, &resource, operation) {
            Ok(underwrap_protocol::policy::PolicyDecision::Allow) => {}
            Ok(underwrap_protocol::policy::PolicyDecision::Deny { reason }) => {
                error!(wrapper = %wrapper_name, credential = %binding.credential_ref, path = %binding.target_path, reason = %reason, "backend policy denied directory channel");
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

pub(crate) async fn process_read_for(
    wrapper_name: &str,
    scope: Option<&str>,
    state: &State,
    peer_uid: u32,
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

    let backend_policy_enabled = match backend_policy_active(&*state.store) {
        Ok(active) => active,
        Err(e) => {
            error!(wrapper = %wrapper_name, err = %e, "policy lookup error");
            return WrapperResponse::Error {
                code: ErrorCode::Internal,
                message: "internal policy error".to_string(),
            };
        }
    };

    if !backend_policy_enabled
        && wrapper_def.unlock.mode != UnlockMode::AlwaysUnlocked
        && let Err(reason) = state.unlock.check(wrapper_name, scope, peer_uid)
    {
        audit::read_denied(wrapper_name, &reason, Some(peer_uid), scope);
        return WrapperResponse::Error {
            code: ErrorCode::Locked,
            message: reason,
        };
    }

    if let Some((credential_ref, access)) =
        wrapper_libsecret_binding(wrapper_def, &state.config.credentials)
    {
        let resource = libsecret_resource(credential_ref);
        let operation = match access {
            AccessMode::ReadOnly => "libsecret.read",
            AccessMode::ReadWrite => "libsecret.readwrite",
        };
        match authorize_operation(&*state.store, wrapper_name, &resource, operation) {
            Ok(underwrap_protocol::policy::PolicyDecision::Allow) => {
                audit::read_granted(wrapper_name, credential_ref, Some(peer_uid), scope);
            }
            Ok(underwrap_protocol::policy::PolicyDecision::Deny { reason }) => {
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
        match authorize_operation(&*state.store, wrapper_name, &resource, "secret.read") {
            Ok(underwrap_protocol::policy::PolicyDecision::Allow) => {}
            Ok(underwrap_protocol::policy::PolicyDecision::Deny { reason }) => {
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
        match authorize_operation(&*state.store, wrapper_name, &resource, operation) {
            Ok(underwrap_protocol::policy::PolicyDecision::Allow) => {
                audit::read_granted(wrapper_name, &binding.credential_ref, Some(peer_uid), scope);
                directories.push(binding);
            }
            Ok(underwrap_protocol::policy::PolicyDecision::Deny { reason }) => {
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
    use crate::unlock::UnlockState;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use underwrap_protocol::config::{
        AccessMode, BackendSocketConfig, CredentialDef, CredentialMode, CredentialUse,
        DaemonConfig, SocketConfig, UnlockPolicy, WrapperDef,
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

    fn state_for(unlock: UnlockPolicy) -> State {
        state_with_store(
            unlock,
            Arc::new(MemStore::with_entries(&[("github/token", b"ghp_test")])),
        )
    }

    fn state_with_store(
        unlock: UnlockPolicy,
        store: Arc<dyn underwrap_store::StorageBackend>,
    ) -> State {
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
                        unlock,
                        socket: PathBuf::from("/tmp/test.sock"),
                        group: "test-group".to_string(),
                        unlock_key: None,
                    },
                )]),
            },
            store,
            allowed_user_uids: std::collections::HashSet::new(),
            unlock: UnlockState::new(),
        }
    }

    #[tokio::test]
    async fn process_read_for_returns_env_and_directory_bindings() {
        let state = state_for(UnlockPolicy::default());
        let resp = process_read_for("gh-authorized", None, &state, 1000).await;
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
    async fn process_read_for_enforces_unlock_state() {
        let state = state_for(UnlockPolicy {
            mode: UnlockMode::Always,
            max_seconds: Some(60),
        });
        let resp = process_read_for("gh-authorized", None, &state, 1000).await;
        match resp {
            WrapperResponse::Error { code, .. } => assert!(matches!(code, ErrorCode::Locked)),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn process_read_for_denies_when_backend_policy_lacks_required_rule() {
        let store: Arc<dyn underwrap_store::StorageBackend> = Arc::new(MemStore::with_policy(
            &[("github/token", b"ghp_test")],
            underwrap_protocol::policy::PolicyDocument::default(),
        ));
        let state = state_with_store(UnlockPolicy::default(), store);
        let resp = process_read_for("gh-authorized", None, &state, 1000).await;
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
        let state = state_with_store(UnlockPolicy::default(), store);
        let resp = process_read_for("gh-authorized", None, &state, 1000).await;
        match resp {
            WrapperResponse::ReadOk { env, directories } => {
                assert_eq!(env.get("GH_TOKEN").unwrap(), &SecretBytes::from("ghp_test"));
                assert_eq!(directories.len(), 1);
            }
            other => panic!("expected ReadOk, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn backend_policy_replaces_local_unlock_when_present() {
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
        let state = state_with_store(
            UnlockPolicy {
                mode: UnlockMode::Always,
                max_seconds: Some(60),
            },
            store,
        );
        let resp = process_read_for("gh-authorized", None, &state, 1000).await;
        assert!(matches!(resp, WrapperResponse::ReadOk { .. }));
    }
}
