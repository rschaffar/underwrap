use tracing::{error, warn};

use ssh_key::public::KeyData;
use underwrap_protocol::config::UnlockMode;
use underwrap_protocol::message::{ErrorCode, WrapperResponse};

use crate::audit;
use crate::server::State;

/// Handle an unlock request according to the wrapper's unlock policy.
pub(crate) fn process_wrapper_unlock(
    wrapper_name: &str,
    scope: Option<&str>,
    seconds: Option<u64>,
    peer_uid: u32,
    state: &State,
) -> WrapperResponse {
    let wrapper_def = match state.config.wrappers.get(wrapper_name) {
        Some(w) => w,
        None => {
            return WrapperResponse::Error {
                code: ErrorCode::NotFound,
                message: format!("unknown wrapper '{wrapper_name}'"),
            };
        }
    };

    match wrapper_def.unlock.mode {
        UnlockMode::AlwaysUnlocked => WrapperResponse::Error {
            code: ErrorCode::Internal,
            message: format!("wrapper '{wrapper_name}' does not require unlock"),
        },
        UnlockMode::Always | UnlockMode::Optional => {
            let max = wrapper_def
                .unlock
                .max_seconds
                .expect("validated maxSeconds");
            if let Some(requested) = seconds
                && requested > max
            {
                return WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: format!(
                        "requested unlock duration {requested}s exceeds maxSeconds {max}s"
                    ),
                };
            }

            if wrapper_def.unlock_key.is_some() {
                let nonce: [u8; 32] = rand::random();
                state
                    .unlock
                    .set_pending_challenge(wrapper_name, scope, peer_uid, &nonce);

                WrapperResponse::Challenge {
                    nonce: nonce.to_vec(),
                }
            } else {
                state.unlock.unlock(wrapper_name, scope, seconds, peer_uid);
                audit::unlocked(wrapper_name, scope, seconds, Some(peer_uid));
                WrapperResponse::UnlockOk
            }
        }
    }
}

/// Handle the second round of unlock: verify the SSH signature over the challenge.
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_wrapper_unlock_response(
    wrapper_name: &str,
    scope: Option<&str>,
    seconds: Option<u64>,
    peer_uid: u32,
    state: &State,
    challenge: &[u8],
    signature: &[u8],
    public_key_blob: &[u8],
) -> WrapperResponse {
    let wrapper_def = match state.config.wrappers.get(wrapper_name) {
        Some(w) => w,
        None => {
            return WrapperResponse::Error {
                code: ErrorCode::NotFound,
                message: format!("unknown wrapper '{wrapper_name}'"),
            };
        }
    };

    let expected_key = match &wrapper_def.unlock_key {
        Some(k) => k,
        None => {
            return WrapperResponse::Error {
                code: ErrorCode::Internal,
                message: "wrapper has no unlock_key configured".to_string(),
            };
        }
    };

    let expected_pk = match ssh_key::PublicKey::from_openssh(expected_key) {
        Ok(k) => k,
        Err(e) => {
            error!(wrapper = %wrapper_name, err = %e, "invalid configured unlock_key");
            return WrapperResponse::Error {
                code: ErrorCode::Internal,
                message: "invalid configured unlock_key".to_string(),
            };
        }
    };

    let caller_pk = match ssh_key::PublicKey::from_bytes(public_key_blob) {
        Ok(k) => k,
        Err(e) => {
            warn!(wrapper = %wrapper_name, err = %e, "invalid public key from caller");
            return WrapperResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "invalid public key".to_string(),
            };
        }
    };

    if expected_pk.key_data() != caller_pk.key_data() {
        audit::auth_denied("unlock", "public key mismatch", Some(peer_uid), None);
        return WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: "public key does not match configured unlock_key".to_string(),
        };
    }

    if !state
        .unlock
        .verify_pending_challenge(wrapper_name, scope, peer_uid, challenge)
    {
        return WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: "invalid or expired challenge nonce".to_string(),
        };
    }

    let ssh_sig = match ssh_key::Signature::try_from(signature) {
        Ok(s) => s,
        Err(e) => {
            warn!(err = %e, "failed to parse SSH signature");
            return WrapperResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "invalid SSH signature format".to_string(),
            };
        }
    };

    use signature::Verifier;
    let valid = match caller_pk.key_data() {
        KeyData::Ed25519(key) => key.verify(challenge, &ssh_sig).is_ok(),
        KeyData::Rsa(key) => key.verify(challenge, &ssh_sig).is_ok(),
        _ => {
            return WrapperResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "unsupported key type (only ed25519 and rsa supported)".to_string(),
            };
        }
    };

    if !valid {
        audit::auth_denied(
            "unlock",
            "signature verification failed",
            Some(peer_uid),
            None,
        );
        return WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: "signature verification failed".to_string(),
        };
    }

    state.unlock.unlock(wrapper_name, scope, seconds, peer_uid);
    audit::unlocked(wrapper_name, scope, seconds, Some(peer_uid));
    WrapperResponse::UnlockOk
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use underwrap_protocol::config::{
        BackendSocketConfig, CredentialDef, CredentialMode, CredentialUse, DaemonConfig,
        SocketConfig, UnlockPolicy, WrapperDef,
    };
    use underwrap_store::StorageBackend;

    struct EmptyStore;
    impl StorageBackend for EmptyStore {
        fn read(
            &self,
            _: &str,
        ) -> anyhow::Result<Option<underwrap_protocol::message::SecretBytes>> {
            Ok(None)
        }
        fn write(&self, _: &str, _: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }
        fn list(&self, _: &str) -> anyhow::Result<Vec<String>> {
            Ok(Vec::new())
        }
        fn delete(&self, _: &str) -> anyhow::Result<bool> {
            Ok(false)
        }
    }

    fn state_for(wrapper: WrapperDef) -> State {
        State {
            config: DaemonConfig {
                sockets: SocketConfig::default(),
                backend: BackendSocketConfig {
                    socket: PathBuf::from("/tmp/backend.sock"),
                    ..Default::default()
                },
                allowed_users: Vec::new(),
                credentials: HashMap::from([(
                    "github/token".to_string(),
                    CredentialDef {
                        mode: CredentialMode::Env {
                            variable: "GH_TOKEN".to_string(),
                        },
                    },
                )]),
                wrappers: HashMap::from([("locked-w".to_string(), wrapper)]),
            },
            store: Arc::new(EmptyStore),
            allowed_user_uids: std::collections::HashSet::new(),
            unlock: crate::unlock::UnlockState::new(),
        }
    }

    fn wrapper(unlock: UnlockPolicy, unlock_key: Option<String>) -> WrapperDef {
        WrapperDef {
            target: PathBuf::from("/usr/bin/true"),
            use_map: HashMap::from([(
                "github/token".to_string(),
                CredentialUse {
                    access: underwrap_protocol::config::AccessMode::ReadOnly,
                },
            )]),
            unlock,
            socket: PathBuf::from("/tmp/test.sock"),
            group: "test-group".to_string(),
            unlock_key,
        }
    }

    #[test]
    fn process_wrapper_unlock_rejects_always_unlocked_and_excessive_duration() {
        let state = state_for(wrapper(UnlockPolicy::default(), None));
        match process_wrapper_unlock("locked-w", None, None, 1000, &state) {
            WrapperResponse::Error { code, .. } => assert!(matches!(code, ErrorCode::Internal)),
            other => panic!("expected Error, got {other:?}"),
        }

        let state = state_for(wrapper(
            UnlockPolicy {
                mode: UnlockMode::Always,
                max_seconds: Some(10),
            },
            None,
        ));
        match process_wrapper_unlock("locked-w", None, Some(20), 1000, &state) {
            WrapperResponse::Error { code, .. } => assert!(matches!(code, ErrorCode::Unauthorized)),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn process_wrapper_unlock_grants_without_key_and_challenges_with_key() {
        let state = state_for(wrapper(
            UnlockPolicy {
                mode: UnlockMode::Always,
                max_seconds: Some(10),
            },
            None,
        ));
        assert!(matches!(
            process_wrapper_unlock("locked-w", Some("s"), Some(5), 1000, &state),
            WrapperResponse::UnlockOk
        ));
        assert!(state.unlock.check("locked-w", Some("s"), 1000).is_ok());

        let state = state_for(wrapper(
            UnlockPolicy { mode: UnlockMode::Optional, max_seconds: Some(10) },
            Some("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGd8eWQ4d2nJ5z2jdK5rj5nP4Jv8xK9w7V6Y8Q1JcKQ9 test".to_string()),
        ));
        match process_wrapper_unlock("locked-w", Some("s"), Some(5), 1000, &state) {
            WrapperResponse::Challenge { nonce } => assert_eq!(nonce.len(), 32),
            other => panic!("expected Challenge, got {other:?}"),
        }
    }

    #[test]
    fn process_wrapper_unlock_response_rejects_missing_or_invalid_key_material() {
        let state = state_for(wrapper(
            UnlockPolicy {
                mode: UnlockMode::Always,
                max_seconds: Some(10),
            },
            None,
        ));
        match process_wrapper_unlock_response(
            "locked-w",
            None,
            Some(5),
            1000,
            &state,
            b"n",
            b"s",
            b"p",
        ) {
            WrapperResponse::Error { code, .. } => assert!(matches!(code, ErrorCode::Internal)),
            other => panic!("expected Error, got {other:?}"),
        }

        let state = state_for(wrapper(
            UnlockPolicy {
                mode: UnlockMode::Optional,
                max_seconds: Some(10),
            },
            Some("not-a-key".to_string()),
        ));
        match process_wrapper_unlock_response(
            "locked-w",
            None,
            Some(5),
            1000,
            &state,
            b"n",
            b"s",
            b"p",
        ) {
            WrapperResponse::Error { code, .. } => assert!(matches!(code, ErrorCode::Internal)),
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
