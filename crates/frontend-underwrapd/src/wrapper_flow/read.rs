use std::collections::HashMap;

use tracing::{error, warn};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::message::{ErrorCode, ReadResourceBinding, SecretBytes, WrapperResponse};
use underwrap_protocol::resource::{
    env_resource, files_resource, libsecret_resource, validate_env_name,
};
use underwrap_store::load_resource_blob;

use crate::audit;
use crate::policy::{
    InvocationDecision, evaluate_invocation, evaluate_invocation_after_approval,
    load_policy_document, writable_for,
};
use crate::server::State;
use crate::wrapper_bindings::{
    wrapper_all_resources, wrapper_env_resources, wrapper_file_bindings, wrapper_libsecret_resource,
};

#[derive(Debug, Clone)]
pub(super) struct ReadResult {
    pub response: WrapperResponse,
    /// Per-resource access map captured during read evaluation,
    /// used by the channel-setup stage to allocate wrapper-resource
    /// channels with the right writability.
    pub resource_access: Vec<(String, AccessMode)>,
}

/// Re-check every grant in the same policy document the mode gate
/// consulted, then build the per-resource bindings the wrapper needs to
/// open one channel per resource. When `approval_satisfied` is true the
/// mode check is bypassed; the grants must still all exist.
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

    // Compute a stable resource list (sorted by id) and emit one audit
    // line per resource. The wrapper-resource channels are opened in
    // this same order so the wrapper-side fd assignment is deterministic.
    let mut resource_ids: Vec<String> = Vec::new();
    for resource_id in wrapper_env_resources(wrapper_def) {
        let _resource = env_resource(resource_id);
        audit::read_granted(wrapper_name, resource_id, Some(peer_uid), scope);
        if !resource_ids.iter().any(|r| r == resource_id) {
            resource_ids.push(resource_id.to_string());
        }
    }
    for projection in wrapper_file_bindings(wrapper_def) {
        let _resource = files_resource(&projection.resource_id);
        audit::read_granted(wrapper_name, &projection.resource_id, Some(peer_uid), scope);
        if !resource_ids.iter().any(|r| r == &projection.resource_id) {
            resource_ids.push(projection.resource_id.clone());
        }
    }
    if let Some(resource_id) = wrapper_libsecret_resource(wrapper_def) {
        let _resource = libsecret_resource(resource_id);
        audit::read_granted(wrapper_name, resource_id, Some(peer_uid), scope);
        if !resource_ids.iter().any(|r| r == resource_id) {
            resource_ids.push(resource_id.to_string());
        }
    }

    // Build the env map by reading each env-eligible resource's blob
    // and copying its `env` section.
    let store = state
        .store
        .as_ref()
        .expect("read flow only runs for enrolled instances");
    let mut env: HashMap<String, SecretBytes> = HashMap::new();
    for resource_id in wrapper_env_resources(wrapper_def) {
        let blob = match load_resource_blob(store.as_ref(), resource_id) {
            Ok(b) => b,
            Err(e) => {
                error!(wrapper = %wrapper_name, resource = %resource_id, err = %e, "resource blob load failed");
                return error_result(ErrorCode::Internal, "internal storage error".to_string());
            }
        };
        for (var_name, material) in &blob.env {
            if let Err(reason) = validate_env_name(var_name) {
                return error_result(
                    ErrorCode::Internal,
                    format!(
                        "resource '{resource_id}' contains invalid env name '{var_name}' for wrapper '{wrapper_name}': {reason}"
                    ),
                );
            }
            if env.insert(var_name.clone(), material.clone()).is_some() {
                return error_result(
                    ErrorCode::Internal,
                    format!(
                        "duplicate env var '{var_name}' across resources for wrapper '{wrapper_name}'"
                    ),
                );
            }
        }
    }

    // One wrapper-resource binding per resource in scope. `libsecret`
    // and `files_target_paths` come from the wrapper config; writability
    // comes from the grant.
    let libsecret_id = wrapper_libsecret_resource(wrapper_def).map(str::to_string);
    let mut files_paths_by_resource: HashMap<String, Vec<String>> = HashMap::new();
    for projection in wrapper_file_bindings(wrapper_def) {
        files_paths_by_resource
            .entry(projection.resource_id.clone())
            .or_default()
            .extend(projection.target_paths.iter().cloned());
    }
    let mut resource_bindings = Vec::with_capacity(resource_ids.len());
    let mut resource_access = Vec::with_capacity(resource_ids.len());
    for resource_id in &resource_ids {
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
        resource_access.push((resource_id.clone(), access));
        resource_bindings.push(ReadResourceBinding {
            resource_id: resource_id.clone(),
            access,
            libsecret: libsecret_id.as_deref() == Some(resource_id.as_str()),
            files_target_paths: files_paths_by_resource
                .remove(resource_id)
                .unwrap_or_default(),
        });
    }

    ReadResult {
        response: WrapperResponse::ReadOk {
            env,
            resources: resource_bindings,
        },
        resource_access,
    }
}

fn error_result(code: ErrorCode, message: String) -> ReadResult {
    ReadResult {
        response: WrapperResponse::Error { code, message },
        resource_access: Vec::new(),
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
    fn process_read_for_returns_env_and_resource_bindings_when_no_policy() {
        let store: Arc<dyn underwrap_store::StorageBackend> =
            Arc::new(MemStore::with_entries(&[(
                "github/token/env/GH_TOKEN",
                b"ghp_test",
            )]));
        let state = state_with_store(store);
        let result = process_read_for("gh-authorized", None, &state, current_uid(), true);
        match result.response {
            WrapperResponse::ReadOk { env, resources } => {
                assert_eq!(env.get("GH_TOKEN").unwrap(), &SecretBytes::from("ghp_test"));
                assert_eq!(resources.len(), 2);
                let by_id: HashMap<&str, &ReadResourceBinding> = resources
                    .iter()
                    .map(|b| (b.resource_id.as_str(), b))
                    .collect();
                let dir = by_id.get("gws/state").unwrap();
                assert_eq!(dir.files_target_paths, vec![".config/gcloud".to_string()]);
                assert!(!dir.libsecret);
                let env = by_id.get("github/token").unwrap();
                assert!(env.files_target_paths.is_empty());
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
