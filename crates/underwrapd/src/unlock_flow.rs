use tracing::{error, warn};

use ssh_key::public::KeyData;
use underwrap_protocol::config::ApprovalMode;
use underwrap_protocol::message::{ErrorCode, WrapperResponse};

use crate::audit;
use crate::policy::backend_policy_active;
use crate::server::State;

/// Handle an approval request according to the wrapper's approval policy.
pub(crate) fn process_wrapper_approval(
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

    match backend_policy_active(&*state.store) {
        Ok(true) => {
            return WrapperResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "backend-managed policy is active; local approval is disabled".to_string(),
            };
        }
        Ok(false) => {}
        Err(e) => {
            error!(wrapper = %wrapper_name, err = %e, "failed to determine backend policy mode");
            return WrapperResponse::Error {
                code: ErrorCode::Internal,
                message: "internal policy error".to_string(),
            };
        }
    }

    match wrapper_def.approval.mode {
        ApprovalMode::AlwaysApproved => WrapperResponse::Error {
            code: ErrorCode::Internal,
            message: format!("wrapper '{wrapper_name}' does not require approval"),
        },
        ApprovalMode::Required | ApprovalMode::OptionalApproval => {
            let max = wrapper_def
                .approval
                .max_seconds
                .expect("validated maxSeconds");
            if let Some(requested) = seconds
                && requested > max
            {
                return WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: format!(
                        "requested approval duration {requested}s exceeds maxSeconds {max}s"
                    ),
                };
            }

            if wrapper_def.approval_key.is_some() {
                let nonce: [u8; 32] = rand::random();
                state
                    .approval
                    .set_pending_challenge(wrapper_name, scope, peer_uid, &nonce);

                WrapperResponse::Challenge {
                    nonce: nonce.to_vec(),
                }
            } else {
                state
                    .approval
                    .approve(wrapper_name, scope, seconds, peer_uid);
                audit::approved(wrapper_name, scope, seconds, Some(peer_uid));
                WrapperResponse::ApprovalOk
            }
        }
    }
}

/// Handle the second round of approval: verify the SSH signature over the challenge.
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_wrapper_approval_response(
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

    match backend_policy_active(&*state.store) {
        Ok(true) => {
            return WrapperResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "backend-managed policy is active; local approval is disabled".to_string(),
            };
        }
        Ok(false) => {}
        Err(e) => {
            error!(wrapper = %wrapper_name, err = %e, "failed to determine backend policy mode");
            return WrapperResponse::Error {
                code: ErrorCode::Internal,
                message: "internal policy error".to_string(),
            };
        }
    }

    let expected_key = match &wrapper_def.approval_key {
        Some(k) => k,
        None => {
            return WrapperResponse::Error {
                code: ErrorCode::Internal,
                message: "wrapper has no approval_key configured".to_string(),
            };
        }
    };

    let expected_pk = match ssh_key::PublicKey::from_openssh(expected_key) {
        Ok(k) => k,
        Err(e) => {
            error!(wrapper = %wrapper_name, err = %e, "invalid configured approval_key");
            return WrapperResponse::Error {
                code: ErrorCode::Internal,
                message: "invalid configured approval_key".to_string(),
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
        audit::auth_denied("approval", "public key mismatch", Some(peer_uid), None);
        return WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: "public key does not match configured approval_key".to_string(),
        };
    }

    if !state
        .approval
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
            "approval",
            "signature verification failed",
            Some(peer_uid),
            None,
        );
        return WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: "signature verification failed".to_string(),
        };
    }

    state
        .approval
        .approve(wrapper_name, scope, seconds, peer_uid);
    audit::approved(wrapper_name, scope, seconds, Some(peer_uid));
    WrapperResponse::ApprovalOk
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use underwrap_protocol::config::{
        ApprovalPolicy, BackendSocketConfig, CredentialDef, CredentialMode, CredentialUse,
        DaemonConfig, SocketConfig, WrapperDef,
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

    struct PolicyStore(Vec<u8>);
    impl StorageBackend for PolicyStore {
        fn read(
            &self,
            secret_id: &str,
        ) -> anyhow::Result<Option<underwrap_protocol::message::SecretBytes>> {
            if secret_id == underwrap_protocol::policy::POLICY_DOCUMENT_SECRET_ID {
                Ok(Some(self.0.as_slice().into()))
            } else {
                Ok(None)
            }
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
        state_with_store(wrapper, Arc::new(EmptyStore))
    }

    fn state_with_store(wrapper: WrapperDef, store: Arc<dyn StorageBackend>) -> State {
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
                wrappers: HashMap::from([("approval-w".to_string(), wrapper)]),
            },
            store,
            allowed_user_uids: std::collections::HashSet::new(),
            approval: crate::approval::ApprovalState::new(),
        }
    }

    fn wrapper(approval: ApprovalPolicy, approval_key: Option<String>) -> WrapperDef {
        WrapperDef {
            target: PathBuf::from("/usr/bin/true"),
            use_map: HashMap::from([(
                "github/token".to_string(),
                CredentialUse {
                    access: underwrap_protocol::config::AccessMode::ReadOnly,
                },
            )]),
            approval,
            socket: PathBuf::from("/tmp/test.sock"),
            group: "test-group".to_string(),
            approval_key,
        }
    }

    #[test]
    fn process_wrapper_approval_rejects_always_approvaled_and_excessive_duration() {
        let state = state_for(wrapper(ApprovalPolicy::default(), None));
        match process_wrapper_approval("approval-w", None, None, 1000, &state) {
            WrapperResponse::Error { code, .. } => assert!(matches!(code, ErrorCode::Internal)),
            other => panic!("expected Error, got {other:?}"),
        }

        let state = state_for(wrapper(
            ApprovalPolicy {
                mode: ApprovalMode::Required,
                max_seconds: Some(10),
            },
            None,
        ));
        match process_wrapper_approval("approval-w", None, Some(20), 1000, &state) {
            WrapperResponse::Error { code, .. } => assert!(matches!(code, ErrorCode::Unauthorized)),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn process_wrapper_approval_grants_without_key_and_challenges_with_key() {
        let state = state_for(wrapper(
            ApprovalPolicy {
                mode: ApprovalMode::Required,
                max_seconds: Some(10),
            },
            None,
        ));
        assert!(matches!(
            process_wrapper_approval("approval-w", Some("s"), Some(5), 1000, &state),
            WrapperResponse::ApprovalOk
        ));
        assert!(state.approval.check("approval-w", Some("s"), 1000).is_ok());

        let state = state_for(wrapper(
            ApprovalPolicy { mode: ApprovalMode::OptionalApproval, max_seconds: Some(10) },
            Some("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGd8eWQ4d2nJ5z2jdK5rj5nP4Jv8xK9w7V6Y8Q1JcKQ9 test".to_string()),
        ));
        match process_wrapper_approval("approval-w", Some("s"), Some(5), 1000, &state) {
            WrapperResponse::Challenge { nonce } => assert_eq!(nonce.len(), 32),
            other => panic!("expected Challenge, got {other:?}"),
        }
    }

    #[test]
    fn process_wrapper_approval_response_rejects_missing_or_invalid_key_material() {
        let state = state_for(wrapper(
            ApprovalPolicy {
                mode: ApprovalMode::Required,
                max_seconds: Some(10),
            },
            None,
        ));
        match process_wrapper_approval_response(
            "approval-w",
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
            ApprovalPolicy {
                mode: ApprovalMode::OptionalApproval,
                max_seconds: Some(10),
            },
            Some("not-a-key".to_string()),
        ));
        match process_wrapper_approval_response(
            "approval-w",
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

    #[test]
    fn process_wrapper_approval_is_disabled_when_backend_policy_is_active() {
        let document = underwrap_protocol::policy::PolicyDocument::default();
        let state = state_with_store(
            wrapper(
                ApprovalPolicy {
                    mode: ApprovalMode::Required,
                    max_seconds: Some(10),
                },
                None,
            ),
            Arc::new(PolicyStore(serde_json::to_vec(&document).unwrap())),
        );
        match process_wrapper_approval("approval-w", None, Some(5), 1000, &state) {
            WrapperResponse::Error { code, message } => {
                assert!(matches!(code, ErrorCode::Unauthorized));
                assert!(message.contains("backend-managed policy is active"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
