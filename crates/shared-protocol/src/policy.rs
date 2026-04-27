//! Central policy model shared by backend and frontend.

use serde::{Deserialize, Serialize};

/// Reserved secret id used to persist the policy document inside a backend store.
pub const POLICY_DOCUMENT_SECRET_ID: &str = "__underwrap/policy/document";

/// Operation name for any write to credential material in the StorageBackend.
///
/// Covers the admin `/v1/admin/secret/write` endpoint and wrapper-initiated
/// writes that come back through the libsecret and directory channels. Reads
/// are gated separately by `secret.read` / `libsecret.read*` /
/// `directory.read*`.
pub const OP_STORE_WRITE: &str = "store.write";

/// Resource identifier used by the [`OP_STORE_WRITE`] gate.
///
/// Mutation is one toggle per credential regardless of how the credential is
/// consumed (env, libsecret, directory). The `cred:` prefix keeps it disjoint
/// from the existing `secret:` / `libsecret:` / `directory:` resource spaces
/// so a read-allow rule can never accidentally permit a write.
pub fn credential_resource(credential_ref: &str) -> String {
    format!("cred:{credential_ref}")
}

/// Result of a policy check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// The operation is allowed outright.
    Allow,
    /// The operation is denied.
    Deny { reason: String },
    /// The operation is allowed only after the operator completes a single-use
    /// SSH-agent signature over a fresh nonce. The caller must run the
    /// approval challenge flow against `approval_key` and re-evaluate with the
    /// proof attached.
    NeedsApproval { approval_key: String },
}

/// Whether a rule allows or denies a request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PolicyEffect {
    Allow,
    Deny,
}

/// Backend-owned runtime mode for a single wrapper inside one instance
/// namespace.
///
/// The mode decides *if* the wrapper can operate at all right now. Rules and
/// grants then decide *which* resources and operations are in scope when the
/// mode permits it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WrapperApprovalMode {
    /// Deny every request for this wrapper.
    Locked,
    /// Allow requests subject to rules/grants. Optionally auto-transitions back
    /// to `Locked` at `until_epoch`.
    Unlocked {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        until_epoch: Option<u64>,
    },
    /// Allow requests subject to rules/grants *only* when the caller presents a
    /// fresh SSH-agent signature verified against `approval_key`. Every single
    /// request needs its own signature.
    PerCallApproval { approval_key: String },
}

impl Default for WrapperApprovalMode {
    fn default() -> Self {
        Self::Unlocked { until_epoch: None }
    }
}

/// Wrapper-level runtime state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WrapperPolicyState {
    pub wrapper: String,
    #[serde(default)]
    pub mode: WrapperApprovalMode,
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
    /// Get the runtime mode for a wrapper. Wrappers without an explicit entry
    /// default to `Unlocked { until_epoch: None }` (permanently unlocked).
    pub fn wrapper_mode(&self, wrapper: &str) -> WrapperApprovalMode {
        self.wrappers
            .iter()
            .find(|state| state.wrapper == wrapper)
            .map(|state| state.mode.clone())
            .unwrap_or_default()
    }

    fn gate_on_mode(&self, wrapper: &str, now_epoch: u64) -> Result<(), PolicyDecision> {
        let entry = self.wrappers.iter().find(|state| state.wrapper == wrapper);
        let mode = entry.map(|e| e.mode.clone()).unwrap_or_default();
        match mode {
            WrapperApprovalMode::Locked => Err(PolicyDecision::Deny {
                reason: entry
                    .and_then(|e| e.reason.clone())
                    .unwrap_or_else(|| format!("wrapper '{wrapper}' is locked")),
            }),
            WrapperApprovalMode::Unlocked { until_epoch } => match until_epoch {
                Some(until) if now_epoch > until => Err(PolicyDecision::Deny {
                    reason: format!(
                        "wrapper '{wrapper}' unlock expired at epoch {until}, now {now_epoch}"
                    ),
                }),
                _ => Ok(()),
            },
            WrapperApprovalMode::PerCallApproval { approval_key } => {
                Err(PolicyDecision::NeedsApproval { approval_key })
            }
        }
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

    /// Evaluate a `(wrapper, resource, operation)` tuple skipping the mode
    /// gate. Used after the caller has already satisfied a per-call approval
    /// challenge for this wrapper and wants to know whether the rule/grant
    /// layer permits the specific operation.
    pub fn evaluate_after_approval_at(
        &self,
        wrapper: &str,
        resource: &str,
        operation: &str,
        now_epoch: u64,
    ) -> PolicyDecision {
        self.rules_and_grants_at(wrapper, resource, operation, now_epoch)
    }

    pub fn evaluate_after_approval(
        &self,
        wrapper: &str,
        resource: &str,
        operation: &str,
    ) -> PolicyDecision {
        let now_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        self.evaluate_after_approval_at(wrapper, resource, operation, now_epoch)
    }

    fn rules_and_grants_at(
        &self,
        wrapper: &str,
        resource: &str,
        operation: &str,
        now_epoch: u64,
    ) -> PolicyDecision {
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

    pub fn evaluate_at(
        &self,
        wrapper: &str,
        resource: &str,
        operation: &str,
        now_epoch: u64,
    ) -> PolicyDecision {
        if let Err(decision) = self.gate_on_mode(wrapper, now_epoch) {
            return decision;
        }
        self.rules_and_grants_at(wrapper, resource, operation, now_epoch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow_rule(wrapper: &str, resource: &str, operation: &str) -> PolicyRule {
        PolicyRule {
            id: format!("{wrapper}:{resource}:{operation}"),
            wrapper: wrapper.to_string(),
            resource: resource.to_string(),
            operation: operation.to_string(),
            effect: PolicyEffect::Allow,
        }
    }

    #[test]
    fn default_wrapper_mode_is_unlocked_forever() {
        let doc = PolicyDocument::default();
        assert_eq!(
            doc.wrapper_mode("anything"),
            WrapperApprovalMode::Unlocked { until_epoch: None }
        );
    }

    #[test]
    fn locked_mode_denies_even_with_matching_rule() {
        let doc = PolicyDocument {
            wrappers: vec![WrapperPolicyState {
                wrapper: "gh-comment".to_string(),
                mode: WrapperApprovalMode::Locked,
                reason: Some("revoked".to_string()),
            }],
            rules: vec![allow_rule(
                "gh-comment",
                "secret:github/token",
                "secret.read",
            )],
            grants: Vec::new(),
        };
        match doc.evaluate_at("gh-comment", "secret:github/token", "secret.read", 100) {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "revoked"),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn unlocked_until_enforces_expiry_lazily() {
        let doc = PolicyDocument {
            wrappers: vec![WrapperPolicyState {
                wrapper: "gh-comment".to_string(),
                mode: WrapperApprovalMode::Unlocked {
                    until_epoch: Some(200),
                },
                reason: None,
            }],
            rules: vec![allow_rule(
                "gh-comment",
                "secret:github/token",
                "secret.read",
            )],
            grants: Vec::new(),
        };
        assert_eq!(
            doc.evaluate_at("gh-comment", "secret:github/token", "secret.read", 199),
            PolicyDecision::Allow
        );
        assert!(matches!(
            doc.evaluate_at("gh-comment", "secret:github/token", "secret.read", 201),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn per_call_approval_returns_needs_approval() {
        let doc = PolicyDocument {
            wrappers: vec![WrapperPolicyState {
                wrapper: "aws-prod".to_string(),
                mode: WrapperApprovalMode::PerCallApproval {
                    approval_key: "ssh-ed25519 AAAA…".to_string(),
                },
                reason: None,
            }],
            rules: vec![allow_rule("aws-prod", "secret:aws/prod", "secret.read")],
            grants: Vec::new(),
        };
        match doc.evaluate_at("aws-prod", "secret:aws/prod", "secret.read", 100) {
            PolicyDecision::NeedsApproval { approval_key } => {
                assert!(approval_key.starts_with("ssh-ed25519"));
            }
            other => panic!("expected NeedsApproval, got {other:?}"),
        }
    }

    #[test]
    fn explicit_deny_rule_takes_precedence_over_allow() {
        let doc = PolicyDocument {
            wrappers: Vec::new(),
            rules: vec![
                allow_rule("gh-comment", "secret:github/token", "secret.read"),
                PolicyRule {
                    id: "deny".to_string(),
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

    #[test]
    fn evaluate_after_approval_skips_the_mode_gate() {
        let doc = PolicyDocument {
            wrappers: vec![WrapperPolicyState {
                wrapper: "aws-prod".to_string(),
                mode: WrapperApprovalMode::PerCallApproval {
                    approval_key: "ssh-ed25519 AAAA".to_string(),
                },
                reason: None,
            }],
            rules: vec![allow_rule("aws-prod", "secret:aws/prod", "secret.read")],
            grants: Vec::new(),
        };
        // With the gate, per-call approval wins over the allow-rule.
        assert!(matches!(
            doc.evaluate_at("aws-prod", "secret:aws/prod", "secret.read", 100),
            PolicyDecision::NeedsApproval { .. }
        ));
        // After approval the allow-rule carries.
        assert_eq!(
            doc.evaluate_after_approval_at("aws-prod", "secret:aws/prod", "secret.read", 100),
            PolicyDecision::Allow
        );
        // Missing rule still denies even after approval.
        assert!(matches!(
            doc.evaluate_after_approval_at("aws-prod", "secret:aws/other", "secret.read", 100),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn document_roundtrips_through_json_with_each_mode() {
        let doc = PolicyDocument {
            wrappers: vec![
                WrapperPolicyState {
                    wrapper: "a".to_string(),
                    mode: WrapperApprovalMode::Locked,
                    reason: Some("r".to_string()),
                },
                WrapperPolicyState {
                    wrapper: "b".to_string(),
                    mode: WrapperApprovalMode::Unlocked {
                        until_epoch: Some(42),
                    },
                    reason: None,
                },
                WrapperPolicyState {
                    wrapper: "c".to_string(),
                    mode: WrapperApprovalMode::PerCallApproval {
                        approval_key: "ssh-ed25519 AAAA".to_string(),
                    },
                    reason: None,
                },
            ],
            rules: Vec::new(),
            grants: Vec::new(),
        };
        let json = serde_json::to_string(&doc).unwrap();
        let parsed: PolicyDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, doc);
    }
}
