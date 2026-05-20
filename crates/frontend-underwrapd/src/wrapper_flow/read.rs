use std::collections::HashMap;

use tracing::{error, warn};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::message::{ErrorCode, SecretBytes, WrapperResponse};
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
use crate::wrapper_bindings::wrapper_file_paths;

#[derive(Debug, Clone)]
pub(super) struct ReadResult {
    pub response: WrapperResponse,
    /// Resource id + access for the single channel the wrapper-flow
    /// will open. `None` when the response is not `ReadOk`.
    pub resource_access: Option<(String, AccessMode)>,
}

/// Re-check the grant the mode gate consulted, then build the single
/// resource binding the wrapper needs to open its channel. When
/// `approval_satisfied` is true the mode check is bypassed; the grant
/// must still exist.
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
    // The daemon addresses the resource by its stable backend id;
    // `wrapper_def.resource` is the operator-facing name from the
    // wrapper config. The register call at startup populated
    // `state.resource_ids` with the resolved id. If the resolution
    // hasn't happened (e.g. auto-create disabled and the resource
    // doesn't exist on the backend yet), fall back to the name so
    // legacy / pre-id-split fixtures keep working — the read will
    // simply fail downstream against an unknown blob.
    let resource_id = state
        .resource_id_for_wrapper(wrapper_name)
        .unwrap_or_else(|| wrapper_def.resource.clone());

    let decision = if approval_satisfied {
        evaluate_invocation_after_approval(
            policy.as_ref(),
            &state.client_instance,
            &logical_name,
            &resource_id,
        )
    } else {
        evaluate_invocation(
            policy.as_ref(),
            &state.client_instance,
            &logical_name,
            &resource_id,
        )
    };

    match decision {
        InvocationDecision::Allow => {}
        InvocationDecision::Deny { reason } => {
            audit::read_denied(wrapper_name, &reason, Some(peer_uid), scope);
            return error_result(ErrorCode::Unauthorized, reason);
        }
        InvocationDecision::NeedsApproval { .. } => {
            // The mode gate runs ahead of process_read_for; reaching
            // this arm means an approval round was expected but wasn't
            // satisfied — treat it as denial.
            let reason = format!("wrapper '{wrapper_name}' requires per-call approval");
            audit::read_denied(wrapper_name, &reason, Some(peer_uid), scope);
            return error_result(ErrorCode::Unauthorized, reason);
        }
    }

    // Emit one audit line per enabled slot. Resource-id stays the same
    // across slots; the slot kind is informational only.
    if wrapper_def.env {
        let _resource = env_resource(&resource_id);
        audit::read_granted(wrapper_name, &resource_id, Some(peer_uid), scope);
    }
    if wrapper_def.files.is_some() {
        let _resource = files_resource(&resource_id);
        audit::read_granted(wrapper_name, &resource_id, Some(peer_uid), scope);
    }
    if wrapper_def.libsecret {
        let _resource = libsecret_resource(&resource_id);
        audit::read_granted(wrapper_name, &resource_id, Some(peer_uid), scope);
    }

    // Build the env map by reading the resource's blob and copying its
    // `env` section when the wrapper opts into the env slot.
    let store = state
        .store
        .as_ref()
        .expect("read flow only runs for enrolled instances");
    let mut env: HashMap<String, SecretBytes> = HashMap::new();
    if wrapper_def.env {
        let blob = match load_resource_blob(store.as_ref(), &resource_id) {
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
            env.insert(var_name.clone(), material.clone());
        }
    }

    let access = if writable_for(
        policy.as_ref(),
        &state.client_instance,
        &logical_name,
        &resource_id,
    ) {
        AccessMode::ReadWrite
    } else {
        AccessMode::ReadOnly
    };
    let files_target_paths = wrapper_file_paths(wrapper_def);
    let libsecret = wrapper_def.libsecret;

    ReadResult {
        response: WrapperResponse::ReadOk {
            env,
            resource_id: resource_id.clone(),
            access,
            libsecret,
            files_target_paths,
        },
        resource_access: Some((resource_id, access)),
    }
}

fn error_result(code: ErrorCode, message: String) -> ReadResult {
    ReadResult {
        response: WrapperResponse::Error { code, message },
        resource_access: None,
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
    use underwrap_protocol::config::{BackendServiceConfig, FilesUse, WrapperDef};
    use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};

    fn state_with_store(store: Arc<dyn underwrap_store::StorageBackend>) -> State {
        State {
            config: underwrap_protocol::config::DaemonConfig {
                backend: BackendServiceConfig::default(),
                wrappers: HashMap::from([(
                    "gh-authorized".to_string(),
                    WrapperDef {
                        target: PathBuf::from("/usr/bin/gh"),
                        logical_name: None,
                        resource: "github/token".to_string(),
                        env: true,
                        files: Some(FilesUse {
                            paths: vec![".config/gh".to_string()],
                        }),
                        libsecret: false,
                        socket: PathBuf::from("/tmp/test.sock"),
                        group: "test-group".to_string(),
                    },
                )]),
            },
            store: Some(store),
            enrollment_url: None,
            client_instance: "dev-laptop".to_string(),
            // Tests pre-bind the wrapper's resource to the same string
            // they use as the resource id in policy permissions / blob
            // writes. This lets the daemon look up by id directly
            // without going through the register flow.
            resource_ids: HashMap::from([(
                "gh-authorized".to_string(),
                "github/token".to_string(),
            )]),
            pending_approvals: approval::PendingApprovals::new(),
        }
    }

    fn current_uid() -> u32 {
        unsafe { libc::getuid() }
    }

    #[test]
    fn process_read_for_returns_env_and_binding_when_no_policy() {
        let store: Arc<dyn underwrap_store::StorageBackend> =
            Arc::new(MemStore::with_entries(&[(
                "github/token/env/GH_TOKEN",
                b"ghp_test",
            )]));
        let state = state_with_store(store);
        let result = process_read_for("gh-authorized", None, &state, current_uid(), true);
        match result.response {
            WrapperResponse::ReadOk {
                env,
                resource_id,
                libsecret,
                files_target_paths,
                ..
            } => {
                assert_eq!(env.get("GH_TOKEN").unwrap(), &SecretBytes::from("ghp_test"));
                assert_eq!(resource_id, "github/token");
                assert!(!libsecret);
                assert_eq!(files_target_paths, vec![".config/gh".to_string()]);
            }
            other => panic!("expected ReadOk, got {other:?}"),
        }
    }

    #[test]
    fn process_read_for_denies_when_permission_missing() {
        let store: Arc<dyn underwrap_store::StorageBackend> =
            Arc::new(MemStore::with_entries_and_policy(
                &[("github/token/env/GH_TOKEN", b"ghp_test")],
                PolicyDocument {
                    permissions: vec![WrapperPermission {
                        client_instance: "other-instance".into(),
                        wrapper: "gh-authorized".into(),
                        resource_id: "github/token".into(),
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
                assert!(message.contains("no permission"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn process_read_for_allows_when_permission_unlocked() {
        let store: Arc<dyn underwrap_store::StorageBackend> =
            Arc::new(MemStore::with_entries_and_policy(
                &[("github/token/env/GH_TOKEN", b"ghp_test")],
                PolicyDocument {
                    permissions: vec![WrapperPermission {
                        client_instance: "dev-laptop".into(),
                        wrapper: "gh-authorized".into(),
                        resource_id: "github/token".into(),
                        mode: WrapperApprovalMode::Unlocked,
                        writable: false,
                        reason: None,
                    }],
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
