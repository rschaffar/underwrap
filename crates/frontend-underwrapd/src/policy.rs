use anyhow::{Context, Result};
use underwrap_protocol::policy::{POLICY_DOCUMENT_SECRET_ID, PolicyDecision, PolicyDocument};
use underwrap_store::StorageBackend;

pub(crate) fn load_policy_document(store: &dyn StorageBackend) -> Result<Option<PolicyDocument>> {
    let Some(material) = store.read(POLICY_DOCUMENT_SECRET_ID)? else {
        return Ok(None);
    };
    let document: PolicyDocument =
        serde_json::from_slice(material.as_ref()).context("parsing backend policy document")?;
    Ok(Some(document))
}

pub(crate) fn authorize_operation(
    store: &dyn StorageBackend,
    wrapper: &str,
    resource: &str,
    operation: &str,
) -> Result<PolicyDecision> {
    let document = load_policy_document(store)?;
    Ok(evaluate_with(&document, wrapper, resource, operation))
}

pub(crate) fn evaluate_with(
    document: &Option<PolicyDocument>,
    wrapper: &str,
    resource: &str,
    operation: &str,
) -> PolicyDecision {
    match document {
        Some(document) => document.evaluate(wrapper, resource, operation),
        None => PolicyDecision::Allow,
    }
}

pub(crate) fn evaluate_after_approval_with(
    document: &Option<PolicyDocument>,
    wrapper: &str,
    resource: &str,
    operation: &str,
) -> PolicyDecision {
    match document {
        Some(document) => document.evaluate_after_approval(wrapper, resource, operation),
        None => PolicyDecision::Allow,
    }
}

pub(crate) use underwrap_protocol::policy::{
    OP_ENV_READ, OP_FILES_READ, OP_FILES_WRITE, OP_LIBSECRET_READ, OP_LIBSECRET_WRITE,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MemStore;
    use underwrap_protocol::policy::{
        PolicyEffect, PolicyRule, WrapperApprovalMode, WrapperPolicyState,
    };

    #[test]
    fn authorize_operation_allows_when_no_policy_document_exists() {
        let store = MemStore::with_policy(None);
        assert_eq!(
            authorize_operation(&store, "gh-comment", "secret:github/token", "secret.read")
                .unwrap(),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn authorize_operation_denies_locked_wrapper() {
        let store = MemStore::with_policy(Some(PolicyDocument {
            wrappers: vec![WrapperPolicyState {
                wrapper: "gh-comment".to_string(),
                mode: WrapperApprovalMode::Locked,
                reason: Some("locked centrally".to_string()),
            }],
            rules: vec![PolicyRule {
                id: "allow-read".to_string(),
                wrapper: "gh-comment".to_string(),
                resource: "secret:github/token".to_string(),
                operation: "secret.read".to_string(),
                effect: PolicyEffect::Allow,
            }],
            grants: Vec::new(),
        }));
        assert!(matches!(
            authorize_operation(&store, "gh-comment", "secret:github/token", "secret.read")
                .unwrap(),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn authorize_operation_surfaces_per_call_approval() {
        let store = MemStore::with_policy(Some(PolicyDocument {
            wrappers: vec![WrapperPolicyState {
                wrapper: "aws-prod".to_string(),
                mode: WrapperApprovalMode::PerCallApproval {
                    approval_key: "ssh-ed25519 AAAA".to_string(),
                },
                reason: None,
            }],
            rules: vec![PolicyRule {
                id: "allow-read".to_string(),
                wrapper: "aws-prod".to_string(),
                resource: "secret:aws/prod".to_string(),
                operation: "secret.read".to_string(),
                effect: PolicyEffect::Allow,
            }],
            grants: Vec::new(),
        }));
        match authorize_operation(&store, "aws-prod", "secret:aws/prod", "secret.read").unwrap() {
            PolicyDecision::NeedsApproval { approval_key } => {
                assert!(approval_key.starts_with("ssh-ed25519"));
            }
            other => panic!("expected NeedsApproval, got {other:?}"),
        }
    }

    #[test]
    fn authorize_operation_allows_matching_rule() {
        let store = MemStore::with_policy(Some(PolicyDocument {
            wrappers: Vec::new(),
            rules: vec![PolicyRule {
                id: "allow-read".to_string(),
                wrapper: "gh-comment".to_string(),
                resource: "secret:github/token".to_string(),
                operation: "secret.read".to_string(),
                effect: PolicyEffect::Allow,
            }],
            grants: Vec::new(),
        }));
        assert_eq!(
            authorize_operation(&store, "gh-comment", "secret:github/token", "secret.read")
                .unwrap(),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn authorize_operation_denies_missing_rule_when_policy_exists() {
        let store = MemStore::with_policy(Some(PolicyDocument::default()));
        assert!(matches!(
            authorize_operation(&store, "gh-comment", "secret:github/token", "secret.read")
                .unwrap(),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn resource_helpers_prefix_expected_kind() {
        assert_eq!(
            underwrap_protocol::resource::env_resource("github/token"),
            "env:github/token"
        );
        assert_eq!(
            underwrap_protocol::resource::libsecret_resource("gh/default"),
            "libsecret:gh/default"
        );
        assert_eq!(
            underwrap_protocol::resource::files_resource("gws/state"),
            "files:gws/state"
        );
    }
}
