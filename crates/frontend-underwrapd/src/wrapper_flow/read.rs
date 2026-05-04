use std::collections::HashMap;

use tracing::{error, warn};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::message::{ErrorCode, SecretBytes, WrapperResponse};
use underwrap_protocol::policy::PolicyDecision;
use underwrap_protocol::resource::{
    env_prefix, env_resource, files_prefix, files_resource, libsecret_prefix, libsecret_resource,
    relative_to_prefix, validate_env_name,
};

use crate::audit;
use crate::policy::{
    OP_ENV_READ, OP_FILES_READ, OP_FILES_WRITE, OP_LIBSECRET_READ, OP_LIBSECRET_WRITE,
    evaluate_after_approval_with, evaluate_with, load_policy_document,
};
use crate::server::State;
use crate::wrapper_bindings::{
    wrapper_env_resources, wrapper_file_bindings, wrapper_libsecret_resource,
};

#[derive(Debug, Clone, Default)]
pub(super) struct BundleSnapshot {
    pub entries: HashMap<String, SecretBytes>,
}

#[derive(Debug, Clone)]
pub(super) struct ReadResult {
    pub response: WrapperResponse,
    pub snapshot: BundleSnapshot,
    pub libsecret_access: Option<(String, AccessMode)>,
}

/// Run the post-gate read: rule/grant authorization and one bundle fetch for
/// every projected slot. When `approval_satisfied` is true, mode gating is
/// bypassed (the caller just proved presence).
pub(super) fn process_read_for(
    wrapper_name: &str,
    scope: Option<&str>,
    state: &State,
    peer_uid: u32,
    approval_satisfied: bool,
) -> ReadResult {
    let wrapper_def = match state.config.wrappers.get(wrapper_name) {
        Some(w) => w,
        None => {
            warn!(wrapper = %wrapper_name, "unknown wrapper");
            return error_result(
                ErrorCode::NotFound,
                format!("unknown wrapper '{wrapper_name}'"),
            );
        }
    };

    let policy = match load_policy_document(&*state.store) {
        Ok(doc) => doc,
        Err(e) => {
            error!(wrapper = %wrapper_name, err = %e, "failed to load policy document");
            return error_result(ErrorCode::Internal, "internal policy error".to_string());
        }
    };

    let authorize = |resource: &str, operation: &str| -> PolicyDecision {
        if approval_satisfied {
            evaluate_after_approval_with(&policy, wrapper_name, resource, operation)
        } else {
            evaluate_with(&policy, wrapper_name, resource, operation)
        }
    };

    let mut prefixes = Vec::new();
    let mut env_resources = Vec::new();
    for resource_id in wrapper_env_resources(wrapper_def) {
        let resource = env_resource(resource_id);
        match require_allowed(
            &authorize,
            wrapper_name,
            &resource,
            OP_ENV_READ,
            Some(peer_uid),
            scope,
        ) {
            Ok(()) => {
                audit::read_granted(wrapper_name, resource_id, Some(peer_uid), scope);
                prefixes.push(env_prefix(resource_id));
                env_resources.push(resource_id.to_string());
            }
            Err(response) => {
                return ReadResult {
                    response,
                    snapshot: BundleSnapshot::default(),
                    libsecret_access: None,
                };
            }
        }
    }

    let mut file_bindings = Vec::new();
    for projection in wrapper_file_bindings(wrapper_def) {
        let resource = files_resource(&projection.resource_id);
        match require_allowed(
            &authorize,
            wrapper_name,
            &resource,
            OP_FILES_READ,
            Some(peer_uid),
            scope,
        ) {
            Ok(()) => {
                let access = access_for_optional_write(&authorize, &resource, OP_FILES_WRITE);
                audit::read_granted(wrapper_name, &projection.resource_id, Some(peer_uid), scope);
                prefixes.push(files_prefix(&projection.resource_id));
                file_bindings.push(projection.into_read_binding(access));
            }
            Err(response) => {
                return ReadResult {
                    response,
                    snapshot: BundleSnapshot::default(),
                    libsecret_access: None,
                };
            }
        }
    }

    let mut libsecret_access = None;
    if let Some(resource_id) = wrapper_libsecret_resource(wrapper_def) {
        let resource = libsecret_resource(resource_id);
        match require_allowed(
            &authorize,
            wrapper_name,
            &resource,
            OP_LIBSECRET_READ,
            Some(peer_uid),
            scope,
        ) {
            Ok(()) => {
                let access = access_for_optional_write(&authorize, &resource, OP_LIBSECRET_WRITE);
                audit::read_granted(wrapper_name, resource_id, Some(peer_uid), scope);
                prefixes.push(libsecret_prefix(resource_id));
                libsecret_access = Some((resource_id.to_string(), access));
            }
            Err(response) => {
                return ReadResult {
                    response,
                    snapshot: BundleSnapshot::default(),
                    libsecret_access: None,
                };
            }
        }
    }

    let snapshot = match state.store.read_prefixes(&prefixes) {
        Ok(entries) => BundleSnapshot { entries },
        Err(e) => {
            error!(wrapper = %wrapper_name, err = %e, "bundle read failed");
            return error_result(ErrorCode::Internal, "internal storage error".to_string());
        }
    };

    let env = match build_env(wrapper_name, &env_resources, &snapshot) {
        Ok(env) => env,
        Err(message) => return error_result(ErrorCode::Internal, message),
    };

    ReadResult {
        response: WrapperResponse::ReadOk {
            env,
            directories: file_bindings,
        },
        snapshot,
        libsecret_access,
    }
}

fn build_env(
    wrapper_name: &str,
    resources: &[String],
    snapshot: &BundleSnapshot,
) -> Result<HashMap<String, SecretBytes>, String> {
    let mut env = HashMap::new();
    for resource_id in resources {
        let prefix = env_prefix(resource_id);
        for (id, material) in &snapshot.entries {
            let Some(var_name) = relative_to_prefix(id, &prefix) else {
                continue;
            };
            validate_env_name(var_name).map_err(|reason| {
                format!(
                    "resource '{resource_id}' contains invalid env name '{var_name}' for wrapper '{wrapper_name}': {reason}"
                )
            })?;
            if env.insert(var_name.to_string(), material.clone()).is_some() {
                return Err(format!(
                    "duplicate env var '{var_name}' across resources for wrapper '{wrapper_name}'"
                ));
            }
        }
    }
    Ok(env)
}

fn require_allowed(
    authorize: &impl Fn(&str, &str) -> PolicyDecision,
    wrapper_name: &str,
    resource: &str,
    operation: &str,
    peer_uid: Option<u32>,
    scope: Option<&str>,
) -> Result<(), WrapperResponse> {
    match authorize(resource, operation) {
        PolicyDecision::Allow => Ok(()),
        PolicyDecision::Deny { reason } => {
            audit::read_denied(wrapper_name, &reason, peer_uid, scope);
            Err(WrapperResponse::Error {
                code: ErrorCode::Unauthorized,
                message: reason,
            })
        }
        PolicyDecision::NeedsApproval { .. } => {
            let reason = format!("wrapper '{wrapper_name}' requires per-call approval");
            audit::read_denied(wrapper_name, &reason, peer_uid, scope);
            Err(WrapperResponse::Error {
                code: ErrorCode::Unauthorized,
                message: reason,
            })
        }
    }
}

fn access_for_optional_write(
    authorize: &impl Fn(&str, &str) -> PolicyDecision,
    resource: &str,
    operation: &str,
) -> AccessMode {
    match authorize(resource, operation) {
        PolicyDecision::Allow => AccessMode::ReadWrite,
        PolicyDecision::Deny { .. } | PolicyDecision::NeedsApproval { .. } => AccessMode::ReadOnly,
    }
}

fn error_result(code: ErrorCode, message: String) -> ReadResult {
    ReadResult {
        response: WrapperResponse::Error { code, message },
        snapshot: BundleSnapshot::default(),
        libsecret_access: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval;
    use crate::test_support::MemStore;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use underwrap_protocol::config::{BackendServiceConfig, FilesUse, ResourceUse, WrapperDef};
    use underwrap_protocol::policy::{PolicyDocument, PolicyEffect, PolicyRule};

    fn state_with_store(store: Arc<dyn underwrap_store::StorageBackend>) -> State {
        State {
            config: underwrap_protocol::config::DaemonConfig {
                backend: BackendServiceConfig::default(),
                catalog_host: "test-host".to_string(),
                catalog_instance: "test-instance".to_string(),
                allowed_users: Vec::new(),
                wrappers: HashMap::from([(
                    "gh-authorized".to_string(),
                    WrapperDef {
                        target: PathBuf::from("/usr/bin/gh"),
                        logical_name: None,
                        resources: HashMap::from([
                            (
                                "github/token".to_string(),
                                ResourceUse {
                                    env: true,
                                    files: None,
                                    libsecret: false,
                                },
                            ),
                            (
                                "gws/state".to_string(),
                                ResourceUse {
                                    env: false,
                                    files: Some(FilesUse {
                                        path: ".config/gcloud".to_string(),
                                    }),
                                    libsecret: false,
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

    #[test]
    fn process_read_for_returns_env_and_directory_bindings() {
        let store: Arc<dyn underwrap_store::StorageBackend> =
            Arc::new(MemStore::with_entries(&[(
                "github/token/env/GH_TOKEN",
                b"ghp_test",
            )]));
        let state = state_with_store(store);
        let result = process_read_for("gh-authorized", None, &state, 1000, true);
        match result.response {
            WrapperResponse::ReadOk { env, directories } => {
                assert_eq!(env.get("GH_TOKEN").unwrap(), &SecretBytes::from("ghp_test"));
                assert_eq!(directories.len(), 1);
                assert_eq!(directories[0].resource_id, "gws/state");
            }
            other => panic!("expected ReadOk, got {other:?}"),
        }
    }

    #[test]
    fn process_read_for_denies_when_backend_policy_lacks_required_rule() {
        let store: Arc<dyn underwrap_store::StorageBackend> =
            Arc::new(MemStore::with_entries_and_policy(
                &[("github/token/env/GH_TOKEN", b"ghp_test")],
                PolicyDocument::default(),
            ));
        let state = state_with_store(store);
        let result = process_read_for("gh-authorized", None, &state, 1000, false);
        match result.response {
            WrapperResponse::Error { code, message } => {
                assert!(matches!(code, ErrorCode::Unauthorized));
                assert!(message.contains("no backend policy rule"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn process_read_for_allows_when_backend_policy_has_required_rules() {
        let store: Arc<dyn underwrap_store::StorageBackend> =
            Arc::new(MemStore::with_entries_and_policy(
                &[("github/token/env/GH_TOKEN", b"ghp_test")],
                PolicyDocument {
                    rules: vec![
                        PolicyRule {
                            id: "env-read".to_string(),
                            wrapper: "gh-authorized".to_string(),
                            resource: "env:github/token".to_string(),
                            operation: "env.read".to_string(),
                            effect: PolicyEffect::Allow,
                        },
                        PolicyRule {
                            id: "files-read".to_string(),
                            wrapper: "gh-authorized".to_string(),
                            resource: "files:gws/state".to_string(),
                            operation: "files.read".to_string(),
                            effect: PolicyEffect::Allow,
                        },
                    ],
                    ..Default::default()
                },
            ));
        let state = state_with_store(store);
        let result = process_read_for("gh-authorized", None, &state, 1000, false);
        assert!(matches!(result.response, WrapperResponse::ReadOk { .. }));
    }

    #[test]
    fn invalid_backend_env_name_fails_closed() {
        let store: Arc<dyn underwrap_store::StorageBackend> =
            Arc::new(MemStore::with_entries(&[(
                "github/token/env/LD_PRELOAD",
                b"evil",
            )]));
        let state = state_with_store(store);
        let result = process_read_for("gh-authorized", None, &state, 1000, true);
        match result.response {
            WrapperResponse::Error { message, .. } => {
                assert!(message.contains("invalid env name"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
