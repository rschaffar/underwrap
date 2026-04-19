use tracing::{error, warn};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::message::{ErrorCode, WrapperResponse};
use underwrap_protocol::policy::PolicyDecision;

use crate::audit;
use crate::policy::{
    authorize_after_approval, authorize_operation, directory_resource, env_resource,
    libsecret_resource,
};
use crate::server::State;
use crate::wrapper_bindings::{
    wrapper_directory_bindings, wrapper_env_credentials, wrapper_libsecret_binding,
};

/// Run the post-gate read: rule/grant authorization for each credential
/// and fetch of any env-bound secret material. When `approval_satisfied`
/// is true, mode gating is bypassed (the caller just proved presence).
pub(super) async fn process_read_for(
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
    use crate::approval;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use underwrap_protocol::config::{
        AccessMode, BackendServiceConfig, CredentialDef, CredentialMode, CredentialUse,
        DaemonConfig, WrapperDef,
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
                backend: BackendServiceConfig::default(),
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
