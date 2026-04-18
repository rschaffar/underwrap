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
    match load_policy_document(store)? {
        Some(document) => Ok(document.evaluate(wrapper, resource, operation)),
        None => Ok(PolicyDecision::Allow),
    }
}

/// Like [`authorize_operation`] but skips the per-wrapper mode gate. Used after
/// the caller has already satisfied a per-call approval challenge.
pub(crate) fn authorize_after_approval(
    store: &dyn StorageBackend,
    wrapper: &str,
    resource: &str,
    operation: &str,
) -> Result<PolicyDecision> {
    match load_policy_document(store)? {
        Some(document) => Ok(document.evaluate_after_approval(wrapper, resource, operation)),
        None => Ok(PolicyDecision::Allow),
    }
}

pub(crate) fn env_resource(credential_ref: &str) -> String {
    format!("secret:{credential_ref}")
}

pub(crate) fn libsecret_resource(credential_ref: &str) -> String {
    format!("libsecret:{credential_ref}")
}

pub(crate) fn directory_resource(credential_ref: &str) -> String {
    format!("directory:{credential_ref}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use underwrap_protocol::message::SecretBytes;
    use underwrap_protocol::policy::{
        PolicyEffect, PolicyRule, WrapperApprovalMode, WrapperPolicyState,
    };

    struct MemStore {
        data: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MemStore {
        fn with_policy(document: Option<PolicyDocument>) -> Self {
            let mut data = HashMap::new();
            if let Some(document) = document {
                data.insert(
                    POLICY_DOCUMENT_SECRET_ID.to_string(),
                    serde_json::to_vec(&document).unwrap(),
                );
            }
            Self {
                data: Mutex::new(data),
            }
        }
    }

    impl StorageBackend for MemStore {
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

        fn list(&self, _prefix: &str) -> anyhow::Result<Vec<String>> {
            Ok(Vec::new())
        }

        fn delete(&self, secret_id: &str) -> anyhow::Result<bool> {
            Ok(self.data.lock().unwrap().remove(secret_id).is_some())
        }
    }

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
        assert_eq!(env_resource("github/token"), "secret:github/token");
        assert_eq!(libsecret_resource("gh/default"), "libsecret:gh/default");
        assert_eq!(directory_resource("gws/state"), "directory:gws/state");
    }
}
