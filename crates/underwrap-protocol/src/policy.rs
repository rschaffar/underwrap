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

/// Store-backed policy document for one underwrap instance namespace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PolicyDocument {
    #[serde(default)]
    pub wrappers: Vec<WrapperPolicyState>,
    #[serde(default)]
    pub rules: Vec<PolicyRule>,
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

    pub fn evaluate(&self, wrapper: &str, resource: &str, operation: &str) -> PolicyDecision {
        if let Err(reason) = self.wrapper_enabled(wrapper) {
            return PolicyDecision::Deny { reason };
        }

        if self.rules.is_empty() {
            return PolicyDecision::Deny {
                reason: format!(
                    "wrapper '{wrapper}' has no backend policy rule for {operation} on {resource}"
                ),
            };
        }

        if self.rules.iter().any(|rule| {
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
                "wrapper '{wrapper}' has no backend policy rule for {operation} on {resource}"
            ),
        }
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
        };
        assert_eq!(
            doc.evaluate("gh-comment", "secret:github/token", "secret.read"),
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
        };
        assert!(matches!(
            doc.evaluate("gh-comment", "secret:github/token", "secret.read"),
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
        };
        assert_eq!(
            doc.evaluate("gh-comment", "secret:github/token", "secret.read"),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn missing_rule_defaults_to_deny() {
        let doc = PolicyDocument::default();
        assert!(matches!(
            doc.evaluate("gh-comment", "secret:github/token", "secret.read"),
            PolicyDecision::Deny { .. }
        ));
    }
}
