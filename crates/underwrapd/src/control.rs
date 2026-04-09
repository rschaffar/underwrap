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
        ControlRequest::StatusAll => {
            let mut wrappers: Vec<String> = state.config.wrappers.keys().cloned().collect();
            wrappers.sort();
            let mut credentials: Vec<String> = state.config.credentials.keys().cloned().collect();
            credentials.sort();
            ControlResponse::StatusAllOk {
                wrappers,
                credentials,
            }
        }
        ControlRequest::StatusTarget { target } => {
            if let Some(status) =
                build_wrapper_status(target, &state.config, &state.unlock, &*state.store)
            {
                ControlResponse::StatusWrapperOk { status }
            } else if let Some(status) =
                build_credential_status(target, &state.config, &*state.store)
            {
                ControlResponse::StatusCredentialOk { status }
            } else {
                ControlResponse::Error {
                    code: ErrorCode::NotFound,
                    message: format!("unknown target '{target}'"),
                }
            }
        }
        ControlRequest::Unlock {
            target,
            scope,
            seconds,
            grant_user,
            grant_all,
        } => {
            let wrapper = match state.config.wrappers.get(target) {
                Some(w) => w,
                None => {
                    return ControlResponse::Error {
                        code: ErrorCode::NotFound,
                        message: format!("unknown wrapper '{target}'"),
                    };
                }
            };
            if let Some(requested) = seconds
                && let Some(max) = wrapper.unlock.max_seconds
                && *requested > max
            {
                return ControlResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: format!(
                        "requested unlock duration {requested}s exceeds maxSeconds {max}s"
                    ),
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

            let mut grant_uids = Vec::new();
            if caller_uid == 0 {
                if *grant_all {
                    grant_uids.extend(state.allowed_user_uids.iter().copied());
                } else if let Some(user) = grant_user {
                    let uid = match resolve_user_uid(user) {
                        Ok(uid) => uid,
                        Err(e) => {
                            return ControlResponse::Error {
                                code: ErrorCode::NotFound,
                                message: format!("unknown user '{user}': {e}"),
                            };
                        }
                    };
                    if !state.allowed_user_uids.is_empty()
                        && !state.allowed_user_uids.contains(&uid)
                    {
                        return ControlResponse::Error {
                            code: ErrorCode::Unauthorized,
                            message: format!("user '{user}' is not allowed for this instance"),
                        };
                    }
                    grant_uids.push(uid);
                } else {
                    return ControlResponse::Error {
                        code: ErrorCode::Unauthorized,
                        message: "root must specify --for-user or --for-all".to_string(),
                    };
                }
            } else {
                if grant_user.is_some() || *grant_all {
                    return ControlResponse::Error {
                        code: ErrorCode::Unauthorized,
                        message: "only root may use --for-user or --for-all".to_string(),
                    };
                }
                grant_uids.push(caller_uid);
            }

            for uid in grant_uids {
                state.unlock.unlock(target, scope.as_deref(), *seconds, uid);
            }
            audit::unlocked(target, scope.as_deref(), *seconds, Some(caller_uid));
            ControlResponse::UnlockOk
        }
        ControlRequest::Lock { target, scope } => {
            state.unlock.lock(target.as_deref(), scope.as_deref());
            audit::locked(target.as_deref(), scope.as_deref(), peer_uid);
            ControlResponse::LockOk
        }
        ControlRequest::Store {
            secret_id,
            material,
        } => {
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

            match state.store.write(secret_id, material.as_ref()) {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unlock::UnlockState;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use underwrap_protocol::config::{
        AccessMode, CredentialDef, CredentialMode, CredentialUse, DaemonConfig, SocketConfig,
        StorageConfig, UnlockMode, UnlockPolicy, WrapperDef,
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

    fn env_credential(variable: &str) -> CredentialDef {
        CredentialDef {
            mode: CredentialMode::Env {
                variable: variable.to_string(),
            },
        }
    }

    fn directory_credential(path: &str) -> CredentialDef {
        CredentialDef {
            mode: CredentialMode::Directory {
                path: path.to_string(),
            },
        }
    }

    fn wrapper_with_uses(uses: &[(&str, CredentialUse)]) -> WrapperDef {
        WrapperDef {
            target: PathBuf::from("/usr/bin/true"),
            use_map: uses
                .iter()
                .map(|(name, usage)| ((*name).to_string(), usage.clone()))
                .collect(),
            unlock: UnlockPolicy::default(),
            socket: PathBuf::from("/tmp/test.sock"),
            group: "test-group".to_string(),
            unlock_key: None,
        }
    }

    fn state_with_config_and_store(
        wrappers: HashMap<String, WrapperDef>,
        credentials: HashMap<String, CredentialDef>,
        allowed_user_uids: std::collections::HashSet<u32>,
        store: MemStore,
    ) -> State {
        State {
            config: DaemonConfig {
                sockets: SocketConfig::default(),
                storage: StorageConfig::Plain {
                    dir: PathBuf::from("/tmp/underwrap-test"),
                },
                allowed_users: Vec::new(),
                credentials,
                wrappers,
            },
            store: Arc::new(store),
            allowed_user_uids,
            unlock: UnlockState::new(),
        }
    }

    #[test]
    fn process_control_status_all_and_status_target_cover_wrapper_and_credential() {
        let wrappers = HashMap::from([(
            "gh-authorized".to_string(),
            wrapper_with_uses(&[(
                "github/token",
                CredentialUse {
                    access: AccessMode::ReadOnly,
                },
            )]),
        )]);
        let credentials = HashMap::from([("github/token".to_string(), env_credential("GH_TOKEN"))]);
        let state = state_with_config_and_store(
            wrappers,
            credentials,
            std::collections::HashSet::new(),
            MemStore::with_entries(&[("github/token", b"ghp_test")]),
        );

        let resp = process_control(&ControlRequest::StatusAll, &state, Some(1000));
        assert_eq!(
            resp,
            ControlResponse::StatusAllOk {
                wrappers: vec!["gh-authorized".to_string()],
                credentials: vec!["github/token".to_string()],
            }
        );

        let resp = process_control(
            &ControlRequest::StatusTarget {
                target: "gh-authorized".to_string(),
            },
            &state,
            Some(1000),
        );
        match resp {
            ControlResponse::StatusWrapperOk { status } => {
                assert_eq!(status.wrapper, "gh-authorized");
                assert_eq!(status.usage.len(), 1);
                assert_eq!(status.usage[0].stored, Some(true));
            }
            other => panic!("expected StatusWrapperOk, got {other:?}"),
        }

        let resp = process_control(
            &ControlRequest::StatusTarget {
                target: "github/token".to_string(),
            },
            &state,
            Some(1000),
        );
        match resp {
            ControlResponse::StatusCredentialOk { status } => {
                assert_eq!(status.credential, "github/token");
                assert!(status.stored);
            }
            other => panic!("expected StatusCredentialOk, got {other:?}"),
        }
    }

    #[test]
    fn process_control_unlock_enforces_authorization_rules() {
        let mut wrapper = wrapper_with_uses(&[(
            "github/token",
            CredentialUse {
                access: AccessMode::ReadOnly,
            },
        )]);
        wrapper.unlock = UnlockPolicy {
            mode: UnlockMode::Always,
            max_seconds: Some(30),
        };

        let base_wrappers = HashMap::from([("locked-w".to_string(), wrapper)]);
        let base_credentials =
            HashMap::from([("github/token".to_string(), env_credential("GH_TOKEN"))]);

        let state = state_with_config_and_store(
            base_wrappers.clone(),
            base_credentials.clone(),
            std::collections::HashSet::from([0]),
            MemStore::with_entries(&[]),
        );

        let resp = process_control(
            &ControlRequest::Unlock {
                target: "locked-w".to_string(),
                scope: None,
                seconds: Some(60),
                grant_user: None,
                grant_all: false,
            },
            &state,
            Some(1000),
        );
        match resp {
            ControlResponse::Error { code, message } => {
                assert_eq!(code, ErrorCode::Unauthorized);
                assert!(message.contains("exceeds maxSeconds"));
            }
            other => panic!("expected Error, got {other:?}"),
        }

        let resp = process_control(
            &ControlRequest::Unlock {
                target: "locked-w".to_string(),
                scope: None,
                seconds: Some(10),
                grant_user: Some("root".to_string()),
                grant_all: false,
            },
            &state,
            Some(1000),
        );
        match resp {
            ControlResponse::Error { code, message } => {
                assert_eq!(code, ErrorCode::Unauthorized);
                assert!(message.contains("only root may use --for-user or --for-all"));
            }
            other => panic!("expected Error, got {other:?}"),
        }

        let resp = process_control(
            &ControlRequest::Unlock {
                target: "locked-w".to_string(),
                scope: None,
                seconds: Some(10),
                grant_user: None,
                grant_all: false,
            },
            &state,
            Some(0),
        );
        match resp {
            ControlResponse::Error { code, message } => {
                assert_eq!(code, ErrorCode::Unauthorized);
                assert!(message.contains("root must specify --for-user or --for-all"));
            }
            other => panic!("expected Error, got {other:?}"),
        }

        let not_allowed_state = state_with_config_and_store(
            base_wrappers,
            base_credentials,
            std::collections::HashSet::from([123456]),
            MemStore::with_entries(&[]),
        );
        let resp = process_control(
            &ControlRequest::Unlock {
                target: "locked-w".to_string(),
                scope: None,
                seconds: Some(10),
                grant_user: Some("root".to_string()),
                grant_all: false,
            },
            &not_allowed_state,
            Some(0),
        );
        match resp {
            ControlResponse::Error { code, message } => {
                assert_eq!(code, ErrorCode::Unauthorized);
                assert!(message.contains("is not allowed for this instance"));
            }
            other => panic!("expected Error, got {other:?}"),
        }

        let mut wrapper = wrapper_with_uses(&[(
            "github/token",
            CredentialUse {
                access: AccessMode::ReadOnly,
            },
        )]);
        wrapper.unlock = UnlockPolicy {
            mode: UnlockMode::Always,
            max_seconds: Some(30),
        };
        let grant_all_state = state_with_config_and_store(
            HashMap::from([("locked-w".to_string(), wrapper)]),
            HashMap::from([("github/token".to_string(), env_credential("GH_TOKEN"))]),
            std::collections::HashSet::from([0]),
            MemStore::with_entries(&[]),
        );
        let resp = process_control(
            &ControlRequest::Unlock {
                target: "locked-w".to_string(),
                scope: Some("session-1".to_string()),
                seconds: Some(10),
                grant_user: None,
                grant_all: true,
            },
            &grant_all_state,
            Some(0),
        );
        assert_eq!(resp, ControlResponse::UnlockOk);
        assert!(
            grant_all_state
                .unlock
                .check("locked-w", Some("session-1"), 0)
                .is_ok()
        );
        assert!(
            grant_all_state
                .unlock
                .check("locked-w", Some("session-1"), 1000)
                .is_err()
        );
    }

    #[test]
    fn process_control_lock_and_store_rejections() {
        let mut wrapper = wrapper_with_uses(&[(
            "gws/state",
            CredentialUse {
                access: AccessMode::ReadOnly,
            },
        )]);
        wrapper.unlock = UnlockPolicy {
            mode: UnlockMode::Always,
            max_seconds: Some(30),
        };

        let state = state_with_config_and_store(
            HashMap::from([("locked-w".to_string(), wrapper)]),
            HashMap::from([
                ("github/token".to_string(), env_credential("GH_TOKEN")),
                (
                    "gws/state".to_string(),
                    directory_credential(".config/gcloud"),
                ),
            ]),
            std::collections::HashSet::new(),
            MemStore::with_entries(&[]),
        );

        let resp = process_control(
            &ControlRequest::Unlock {
                target: "locked-w".to_string(),
                scope: Some("session-1".to_string()),
                seconds: Some(10),
                grant_user: None,
                grant_all: false,
            },
            &state,
            Some(1000),
        );
        assert_eq!(resp, ControlResponse::UnlockOk);
        assert!(
            state
                .unlock
                .check("locked-w", Some("session-1"), 1000)
                .is_ok()
        );

        let resp = process_control(
            &ControlRequest::Lock {
                target: Some("locked-w".to_string()),
                scope: Some("session-1".to_string()),
            },
            &state,
            Some(1000),
        );
        assert_eq!(resp, ControlResponse::LockOk);
        assert!(
            state
                .unlock
                .check("locked-w", Some("session-1"), 1000)
                .is_err()
        );

        let resp = process_control(
            &ControlRequest::Store {
                secret_id: "gws/state".to_string(),
                material: "data".into(),
            },
            &state,
            Some(1000),
        );
        match resp {
            ControlResponse::Error { code, message } => {
                assert_eq!(code, ErrorCode::Unauthorized);
                assert!(message.contains("not an env credential"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
