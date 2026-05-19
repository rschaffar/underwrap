use std::collections::HashMap;

use tracing::{error, warn};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::message::{ErrorCode, SecretBytes, WrapperResponse};
use underwrap_protocol::resource::{
    env_prefix, env_resource, files_resource, libsecret_resource, relative_to_prefix,
    validate_env_name,
};
use underwrap_store::{expand_blob_to_entries, load_resource_blob};

use crate::audit;
use crate::policy::{
    InvocationDecision, evaluate_invocation, evaluate_invocation_after_approval,
    load_policy_document, writable_for,
};
use crate::server::State;
use crate::wrapper_bindings::{
    wrapper_all_resources, wrapper_env_resources, wrapper_file_bindings, wrapper_libsecret_resource,
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

/// Re-check every grant in the same policy document the mode gate
/// consulted, then fetch every resource the wrapper declares. When
/// `approval_satisfied` is true the mode check is bypassed; the grants
/// must still all exist.
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

    let policy = match load_policy_document(
        state
            .store
            .as_ref()
            .expect("read flow only runs for enrolled instances")
            .as_ref(),
    ) {
        Ok(doc) => doc,
        Err(e) => {
            error!(wrapper = %wrapper_name, err = %e, "failed to load policy document");
            return error_result(ErrorCode::Internal, "internal policy error".to_string());
        }
    };

    let logical_name = wrapper_def
        .logical_name
        .clone()
        .unwrap_or_else(|| wrapper_name.to_string());
    let resources = wrapper_all_resources(wrapper_def);

    let decision = if approval_satisfied {
        evaluate_invocation_after_approval(
            policy.as_ref(),
            &state.client_instance,
            &logical_name,
            &resources,
        )
    } else {
        evaluate_invocation(
            policy.as_ref(),
            &state.client_instance,
            &logical_name,
            &resources,
        )
    };

    match decision {
        InvocationDecision::Allow => {}
        InvocationDecision::Deny { reason } => {
            audit::read_denied(wrapper_name, &reason, Some(peer_uid), scope);
            return error_result(ErrorCode::Unauthorized, reason);
        }
        InvocationDecision::NeedsApproval { .. } => {
            // The mode gate runs ahead of process_read_for, so reaching
            // this arm means an approval round was expected but wasn't
            // satisfied — treat it as denial.
            let reason = format!("wrapper '{wrapper_name}' requires per-call approval");
            audit::read_denied(wrapper_name, &reason, Some(peer_uid), scope);
            return error_result(ErrorCode::Unauthorized, reason);
        }
    }

    // The grants pass. Each resource's per-grant `writable` flag
    // decides whether the wrapper-side mutable slot (`files` /
    // `libsecret`) is opened for writes.
    let mut resource_ids: Vec<String> = Vec::new();
    let mut env_resources = Vec::new();
    for resource_id in wrapper_env_resources(wrapper_def) {
        let _resource = env_resource(resource_id);
        audit::read_granted(wrapper_name, resource_id, Some(peer_uid), scope);
        if !resource_ids.iter().any(|r| r == resource_id) {
            resource_ids.push(resource_id.to_string());
        }
        env_resources.push(resource_id.to_string());
    }

    let mut file_bindings = Vec::new();
    for projection in wrapper_file_bindings(wrapper_def) {
        let _resource = files_resource(&projection.resource_id);
        audit::read_granted(wrapper_name, &projection.resource_id, Some(peer_uid), scope);
        if !resource_ids.iter().any(|r| r == &projection.resource_id) {
            resource_ids.push(projection.resource_id.clone());
        }
        let access = if writable_for(
            policy.as_ref(),
            &state.client_instance,
            &logical_name,
            &projection.resource_id,
        ) {
            AccessMode::ReadWrite
        } else {
            AccessMode::ReadOnly
        };
        file_bindings.push(projection.into_read_binding(access));
    }

    let mut libsecret_access = None;
    if let Some(resource_id) = wrapper_libsecret_resource(wrapper_def) {
        let _resource = libsecret_resource(resource_id);
        audit::read_granted(wrapper_name, resource_id, Some(peer_uid), scope);
        if !resource_ids.iter().any(|r| r == resource_id) {
            resource_ids.push(resource_id.to_string());
        }
        let access = if writable_for(
            policy.as_ref(),
            &state.client_instance,
            &logical_name,
            resource_id,
        ) {
            AccessMode::ReadWrite
        } else {
            AccessMode::ReadOnly
        };
        libsecret_access = Some((resource_id.to_string(), access));
    }

    let store = state
        .store
        .as_ref()
        .expect("read flow only runs for enrolled instances");
    let mut snapshot_entries: HashMap<String, SecretBytes> = HashMap::new();
    for resource_id in &resource_ids {
        let blob = match load_resource_blob(store.as_ref(), resource_id) {
            Ok(blob) => blob,
            Err(e) => {
                error!(wrapper = %wrapper_name, resource = %resource_id, err = %e, "resource blob load failed");
                return error_result(ErrorCode::Internal, "internal storage error".to_string());
            }
        };
        let entries = match expand_blob_to_entries(resource_id, &blob) {
            Ok(entries) => entries,
            Err(e) => {
                error!(wrapper = %wrapper_name, resource = %resource_id, err = %e, "resource blob decode failed");
                return error_result(ErrorCode::Internal, "internal storage error".to_string());
            }
        };
        snapshot_entries.extend(entries);
    }
    let snapshot = BundleSnapshot {
        entries: snapshot_entries,
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
    use underwrap_protocol::policy::{AgentGrant, PolicyDocument, WrapperApprovalMode};

    fn state_with_store(store: Arc<dyn underwrap_store::StorageBackend>) -> State {
        State {
            config: underwrap_protocol::config::DaemonConfig {
                backend: BackendServiceConfig::default(),
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
                                        paths: vec![".config/gcloud".to_string()],
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
            store: Some(store),
            enrollment_url: None,
            client_instance: "dev-laptop".to_string(),
            pending_approvals: approval::PendingApprovals::new(),
        }
    }

    fn current_uid() -> u32 {
        unsafe { libc::getuid() }
    }

    #[test]
    fn process_read_for_returns_env_and_directory_bindings_when_no_policy() {
        // Empty document = fresh store = permissive (first-launch
        // greenfield default).
        let store: Arc<dyn underwrap_store::StorageBackend> =
            Arc::new(MemStore::with_entries(&[(
                "github/token/env/GH_TOKEN",
                b"ghp_test",
            )]));
        let state = state_with_store(store);
        let result = process_read_for("gh-authorized", None, &state, current_uid(), true);
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
    fn process_read_for_denies_when_grant_missing() {
        let store: Arc<dyn underwrap_store::StorageBackend> =
            Arc::new(MemStore::with_entries_and_policy(
                &[("github/token/env/GH_TOKEN", b"ghp_test")],
                PolicyDocument {
                    grants: vec![AgentGrant {
                        client_instance: "dev-laptop".into(),
                        wrapper: "gh-authorized".into(),
                        resource: "github/token".into(),
                        mode: WrapperApprovalMode::Unlocked,
                        writable: false,
                        reason: None,
                    }],
                },
            ));
        let state = state_with_store(store);
        // Missing grant for "gws/state" → invocation denied.
        let result = process_read_for("gh-authorized", None, &state, current_uid(), false);
        match result.response {
            WrapperResponse::Error { code, message } => {
                assert!(matches!(code, ErrorCode::Unauthorized));
                assert!(message.contains("no grant"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn process_read_for_allows_when_all_grants_unlocked() {
        let store: Arc<dyn underwrap_store::StorageBackend> =
            Arc::new(MemStore::with_entries_and_policy(
                &[("github/token/env/GH_TOKEN", b"ghp_test")],
                PolicyDocument {
                    grants: vec![
                        AgentGrant {
                            client_instance: "dev-laptop".into(),
                            wrapper: "gh-authorized".into(),
                            resource: "github/token".into(),
                            mode: WrapperApprovalMode::Unlocked,
                            writable: false,
                            reason: None,
                        },
                        AgentGrant {
                            client_instance: "dev-laptop".into(),
                            wrapper: "gh-authorized".into(),
                            resource: "gws/state".into(),
                            mode: WrapperApprovalMode::Unlocked,
                            writable: false,
                            reason: None,
                        },
                    ],
                },
            ));
        let state = state_with_store(store);
        let result = process_read_for("gh-authorized", None, &state, current_uid(), false);
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
        let result = process_read_for("gh-authorized", None, &state, current_uid(), true);
        match result.response {
            WrapperResponse::Error { message, .. } => {
                assert!(message.contains("invalid env name"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
