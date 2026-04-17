//! Central policy model shared by backend and frontend.

use serde::{Deserialize, Serialize};

/// Reserved secret id used to persist the policy document inside a backend store.
pub const POLICY_DOCUMENT_SECRET_ID: &str = "__underwrap/policy/document";

/// Result of a policy check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny { reason: String },
}

/// Whether a rule allows or denies a request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PolicyEffect {
    Allow,
    Deny,
}

/// Wrapper-level runtime state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WrapperPolicyState {
    pub wrapper: String,
    pub enabled: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

/// One policy rule for one wrapper/resource/operation tuple.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyRule {
    pub id: String,
    pub wrapper: String,
    pub resource: String,
    pub operation: String,
    pub effect: PolicyEffect,
}

/// A time-bounded runtime grant for one wrapper/resource/operation tuple.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyGrant {
    pub id: String,
    pub wrapper: String,
    pub resource: String,
    pub operation: String,
    pub effect: PolicyEffect,
    #[serde(default)]
    pub starts_at_epoch: Option<u64>,
    #[serde(default)]
    pub expires_at_epoch: Option<u64>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Store-backed policy document for one underwrap instance namespace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PolicyDocument {
    #[serde(default)]
    pub wrappers: Vec<WrapperPolicyState>,
    #[serde(default)]
    pub rules: Vec<PolicyRule>,
    #[serde(default)]
    pub grants: Vec<PolicyGrant>,
}

impl PolicyDocument {
    pub fn wrapper_enabled(&self, wrapper: &str) -> std::result::Result<(), String> {
        if let Some(state) = self.wrappers.iter().find(|state| state.wrapper == wrapper)
            && !state.enabled
        {
            return Err(state
                .reason
                .clone()
                .unwrap_or_else(|| format!("wrapper '{wrapper}' is disabled by backend policy")));
        }
        Ok(())
    }

    fn grant_active(grant: &PolicyGrant, now_epoch: u64) -> bool {
        if let Some(starts_at_epoch) = grant.starts_at_epoch
            && now_epoch < starts_at_epoch
        {
            return false;
        }
        if let Some(expires_at_epoch) = grant.expires_at_epoch
            && now_epoch > expires_at_epoch
        {
            return false;
        }
        true
    }

    pub fn evaluate_at(
        &self,
        wrapper: &str,
        resource: &str,
        operation: &str,
        now_epoch: u64,
    ) -> PolicyDecision {
        if let Err(reason) = self.wrapper_enabled(wrapper) {
            return PolicyDecision::Deny { reason };
        }

        if self.grants.iter().any(|grant| {
            grant.wrapper == wrapper
                && grant.resource == resource
                && grant.operation == operation
                && grant.effect == PolicyEffect::Deny
                && Self::grant_active(grant, now_epoch)
        }) || self.rules.iter().any(|rule| {
            rule.wrapper == wrapper
                && rule.resource == resource
                && rule.operation == operation
                && rule.effect == PolicyEffect::Deny
        }) {
            return PolicyDecision::Deny {
                reason: format!(
                    "wrapper '{wrapper}' is denied {operation} on {resource} by backend policy"
                ),
            };
        }

        if self.grants.iter().any(|grant| {
            grant.wrapper == wrapper
                && grant.resource == resource
                && grant.operation == operation
                && grant.effect == PolicyEffect::Allow
                && Self::grant_active(grant, now_epoch)
        }) {
            return PolicyDecision::Allow;
        }

        if self.rules.iter().any(|rule| {
            rule.wrapper == wrapper
                && rule.resource == resource
                && rule.operation == operation
                && rule.effect == PolicyEffect::Allow
        }) {
            return PolicyDecision::Allow;
        }

        PolicyDecision::Deny {
            reason: format!(
                "wrapper '{wrapper}' has no backend policy rule or active grant for {operation} on {resource}"
            ),
        }
    }

    pub fn evaluate(&self, wrapper: &str, resource: &str, operation: &str) -> PolicyDecision {
        let now_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        self.evaluate_at(wrapper, resource, operation, now_epoch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_decision_variants_are_constructible() {
        let allow = PolicyDecision::Allow;
        assert!(matches!(allow, PolicyDecision::Allow));

        let deny = PolicyDecision::Deny {
            reason: "locked".to_string(),
        };
        match deny {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "locked"),
            PolicyDecision::Allow => panic!("expected deny"),
        }
    }

    #[test]
    fn disabled_wrapper_denies_even_without_rules() {
        let doc = PolicyDocument {
            wrappers: vec![WrapperPolicyState {
                wrapper: "gh-comment".to_string(),
                enabled: false,
                reason: Some("temporarily revoked".to_string()),
            }],
            rules: Vec::new(),
            grants: Vec::new(),
        };
        assert_eq!(
            doc.evaluate_at("gh-comment", "secret:github/token", "secret.read", 100),
            PolicyDecision::Deny {
                reason: "temporarily revoked".to_string()
            }
        );
    }

    #[test]
    fn explicit_deny_takes_precedence_over_allow() {
        let doc = PolicyDocument {
            wrappers: Vec::new(),
            rules: vec![
                PolicyRule {
                    id: "1".to_string(),
                    wrapper: "gh-comment".to_string(),
                    resource: "secret:github/token".to_string(),
                    operation: "secret.read".to_string(),
                    effect: PolicyEffect::Allow,
                },
                PolicyRule {
                    id: "2".to_string(),
                    wrapper: "gh-comment".to_string(),
                    resource: "secret:github/token".to_string(),
                    operation: "secret.read".to_string(),
                    effect: PolicyEffect::Deny,
                },
            ],
            grants: Vec::new(),
        };
        assert!(matches!(
            doc.evaluate_at("gh-comment", "secret:github/token", "secret.read", 100),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn explicit_allow_succeeds() {
        let doc = PolicyDocument {
            wrappers: Vec::new(),
            rules: vec![PolicyRule {
                id: "1".to_string(),
                wrapper: "gh-comment".to_string(),
                resource: "secret:github/token".to_string(),
                operation: "secret.read".to_string(),
                effect: PolicyEffect::Allow,
            }],
            grants: Vec::new(),
        };
        assert_eq!(
            doc.evaluate_at("gh-comment", "secret:github/token", "secret.read", 100),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn missing_rule_defaults_to_deny() {
        let doc = PolicyDocument::default();
        assert!(matches!(
            doc.evaluate_at("gh-comment", "secret:github/token", "secret.read", 100),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn active_allow_grant_overrides_missing_rule_and_expiry_is_enforced() {
        let doc = PolicyDocument {
            wrappers: Vec::new(),
            rules: Vec::new(),
            grants: vec![PolicyGrant {
                id: "g1".to_string(),
                wrapper: "gh-comment".to_string(),
                resource: "secret:github/token".to_string(),
                operation: "secret.read".to_string(),
                effect: PolicyEffect::Allow,
                starts_at_epoch: Some(100),
                expires_at_epoch: Some(200),
                reason: Some("temporary approval".to_string()),
            }],
        };
        assert!(matches!(
            doc.evaluate_at("gh-comment", "secret:github/token", "secret.read", 50),
            PolicyDecision::Deny { .. }
        ));
        assert_eq!(
            doc.evaluate_at("gh-comment", "secret:github/token", "secret.read", 150),
            PolicyDecision::Allow
        );
        assert!(matches!(
            doc.evaluate_at("gh-comment", "secret:github/token", "secret.read", 201),
            PolicyDecision::Deny { .. }
        ));
    }
}
