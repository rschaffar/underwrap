//! Central policy model shared by backend and frontend.
//!
//! A grant is keyed on `(client_instance, wrapper, resource)` — the only
//! client-side identifier the backend actually authenticates is the
//! mTLS-bound runtime client instance id. Everything else the daemon
//! would have self-reported (host, local user, instance name from the
//! YAML config) is gone.
//!
//! Each grant carries:
//!
//! - a [`WrapperApprovalMode`] (`Locked`, `Unlocked`,
//!   `UnlockedUntil`, `PerCallApproval`) deciding *if* the grant is
//!   usable right now;
//! - a `writable` flag controlling wrapper-side writes through the
//!   secretsd channel for the `files` and `libsecret` slots (`env`
//!   has no write path);
//! - an optional operator-facing `reason` surfaced in the matrix UI.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Reserved secret id used to persist the policy document inside a backend store.
pub const POLICY_DOCUMENT_SECRET_ID: &str = "__underwrap/policy/document";

/// Result of checking a grant's mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// The grant is active; the wrapper may operate.
    Allow,
    /// The grant denies this invocation.
    Deny { reason: String },
    /// The grant requires a fresh ssh-agent signature over a backend-issued
    /// nonce, verified against `approval_key`.
    NeedsApproval { approval_key: String },
}

/// Runtime mode for one [`AgentGrant`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[derive(Default)]
pub enum WrapperApprovalMode {
    /// Deny every invocation through this grant.
    Locked,
    /// Allow every invocation through this grant.
    #[default]
    Unlocked,
    /// Allow invocations until `until_epoch`; deny afterwards. The expiry is
    /// checked lazily on each request — no server-side sweeper.
    UnlockedUntil { until_epoch: u64 },
    /// Allow invocations only when the caller presents a fresh SSH-agent
    /// signature verified against `approval_key`. Every request needs its
    /// own signature.
    PerCallApproval { approval_key: String },
}

/// One per-store agent grant.
///
/// The tuple `(client_instance, wrapper, resource)` is the unique key;
/// persistence layers MUST reject duplicates with the stable error code
/// `duplicate_grant`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AgentGrant {
    pub client_instance: String,
    pub wrapper: String,
    pub resource: String,
    #[serde(default)]
    pub mode: WrapperApprovalMode,
    /// Whether the wrapper may write to this resource through the
    /// secretsd channel. Applies to the `files` and `libsecret` slots;
    /// `env` has no wrapper-side write path. Defaults to `false`.
    #[serde(default)]
    pub writable: bool,
    /// Operator-facing free-form note. Surfaced in the matrix UI tooltip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
}

/// Store-backed policy document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, TS)]
#[ts(export)]
pub struct PolicyDocument {
    /// The flat list of agent grants. One per
    /// `(client_instance, wrapper, resource)` tuple.
    #[serde(default)]
    pub grants: Vec<AgentGrant>,
}

/// The three secret "slots" a wrapper resource carries. The first two
/// have a wrapper-side write path and honour the per-grant `writable`
/// flag; `Env` is daemon→target only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretSlot {
    Env,
    Files,
    Libsecret,
}

impl PolicyDocument {
    /// Look up the grant for `(client_instance, wrapper, resource)`.
    pub fn find_grant(
        &self,
        client_instance: &str,
        wrapper: &str,
        resource: &str,
    ) -> Option<&AgentGrant> {
        self.grants.iter().find(|g| {
            g.client_instance == client_instance && g.wrapper == wrapper && g.resource == resource
        })
    }

    /// Evaluate the daemon's gate for a given grant tuple.
    ///
    /// - `None` → no matching grant exists.
    /// - `Some(Allow)` → caller may proceed.
    /// - `Some(Deny { reason })` → caller is rejected.
    /// - `Some(NeedsApproval { approval_key })` → caller must run the
    ///   per-call ssh-agent challenge flow.
    pub fn evaluate_grant_at(
        &self,
        client_instance: &str,
        wrapper: &str,
        resource: &str,
        now_epoch: u64,
    ) -> Option<PolicyDecision> {
        let grant = self.find_grant(client_instance, wrapper, resource)?;
        Some(decide(grant, now_epoch))
    }

    pub fn evaluate_grant(
        &self,
        client_instance: &str,
        wrapper: &str,
        resource: &str,
    ) -> Option<PolicyDecision> {
        self.evaluate_grant_at(client_instance, wrapper, resource, now_epoch_secs())
    }

    /// After the caller has satisfied a per-call approval round, the mode
    /// gate is already known to allow; this returns `Allow` for the
    /// existing grant tuple and `None` if the grant was deleted between
    /// the approval round and the second pass.
    pub fn evaluate_grant_after_approval(
        &self,
        client_instance: &str,
        wrapper: &str,
        resource: &str,
    ) -> Option<PolicyDecision> {
        self.find_grant(client_instance, wrapper, resource)
            .map(|_| PolicyDecision::Allow)
    }

    /// True when the wrapper is permitted to write to `resource` (any
    /// slot with a wrapper-side write path). Defaults to `false`
    /// (no grant → not writable).
    pub fn writable_for(&self, client_instance: &str, wrapper: &str, resource: &str) -> bool {
        self.find_grant(client_instance, wrapper, resource)
            .map(|g| g.writable)
            .unwrap_or(false)
    }
}

fn decide(grant: &AgentGrant, now_epoch: u64) -> PolicyDecision {
    match grant.mode.clone() {
        WrapperApprovalMode::Locked => PolicyDecision::Deny {
            reason: grant
                .reason
                .clone()
                .unwrap_or_else(|| format!("grant for '{}' is locked", grant.wrapper)),
        },
        WrapperApprovalMode::Unlocked => PolicyDecision::Allow,
        WrapperApprovalMode::UnlockedUntil { until_epoch } => {
            if now_epoch > until_epoch {
                PolicyDecision::Deny {
                    reason: format!(
                        "grant for '{}'/'{}' expired at epoch {until_epoch}, now {now_epoch}",
                        grant.wrapper, grant.resource
                    ),
                }
            } else {
                PolicyDecision::Allow
            }
        }
        WrapperApprovalMode::PerCallApproval { approval_key } => {
            PolicyDecision::NeedsApproval { approval_key }
        }
    }
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_grant(mode: WrapperApprovalMode) -> PolicyDocument {
        PolicyDocument {
            grants: vec![AgentGrant {
                client_instance: "dev-laptop".to_string(),
                wrapper: "gh-comment".to_string(),
                resource: "github/token".to_string(),
                mode,
                writable: false,
                reason: None,
            }],
        }
    }

    #[test]
    fn missing_grant_returns_none() {
        let doc = PolicyDocument::default();
        assert!(doc.evaluate_grant_at("c", "w", "r", 100).is_none());
    }

    #[test]
    fn locked_grant_denies_with_reason() {
        let mut doc = doc_with_grant(WrapperApprovalMode::Locked);
        doc.grants[0].reason = Some("revoked".into());
        match doc
            .evaluate_grant_at("dev-laptop", "gh-comment", "github/token", 100)
            .unwrap()
        {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "revoked"),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn unlocked_grant_allows() {
        let doc = doc_with_grant(WrapperApprovalMode::Unlocked);
        assert_eq!(
            doc.evaluate_grant_at("dev-laptop", "gh-comment", "github/token", 100)
                .unwrap(),
            PolicyDecision::Allow,
        );
    }

    #[test]
    fn unlocked_until_grant_allows_then_denies_lazily() {
        let doc = doc_with_grant(WrapperApprovalMode::UnlockedUntil { until_epoch: 200 });
        assert_eq!(
            doc.evaluate_grant_at("dev-laptop", "gh-comment", "github/token", 199)
                .unwrap(),
            PolicyDecision::Allow,
        );
        assert!(matches!(
            doc.evaluate_grant_at("dev-laptop", "gh-comment", "github/token", 201)
                .unwrap(),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn per_call_approval_returns_needs_approval() {
        let doc = doc_with_grant(WrapperApprovalMode::PerCallApproval {
            approval_key: "ssh-ed25519 AAAA".into(),
        });
        match doc
            .evaluate_grant_at("dev-laptop", "gh-comment", "github/token", 100)
            .unwrap()
        {
            PolicyDecision::NeedsApproval { approval_key } => {
                assert!(approval_key.starts_with("ssh-ed25519"));
            }
            other => panic!("expected NeedsApproval, got {other:?}"),
        }
    }

    #[test]
    fn evaluate_after_approval_allows_when_grant_still_exists() {
        let doc = doc_with_grant(WrapperApprovalMode::PerCallApproval {
            approval_key: "k".into(),
        });
        assert_eq!(
            doc.evaluate_grant_after_approval("dev-laptop", "gh-comment", "github/token")
                .unwrap(),
            PolicyDecision::Allow,
        );
        let empty = PolicyDocument::default();
        assert!(
            empty
                .evaluate_grant_after_approval("dev-laptop", "gh-comment", "github/token")
                .is_none()
        );
    }

    #[test]
    fn writable_defaults_to_false_without_grant() {
        let doc = PolicyDocument::default();
        assert!(!doc.writable_for("dev-laptop", "gh", "r"));
    }

    #[test]
    fn writable_honours_per_grant_flag() {
        let mut doc = doc_with_grant(WrapperApprovalMode::Unlocked);
        assert!(!doc.writable_for("dev-laptop", "gh-comment", "github/token"));
        doc.grants[0].writable = true;
        assert!(doc.writable_for("dev-laptop", "gh-comment", "github/token"));
    }

    #[test]
    fn document_roundtrips_through_json_with_each_mode() {
        let doc = PolicyDocument {
            grants: vec![
                AgentGrant {
                    client_instance: "a".into(),
                    wrapper: "w1".into(),
                    resource: "r1".into(),
                    mode: WrapperApprovalMode::Locked,
                    writable: false,
                    reason: Some("r".into()),
                },
                AgentGrant {
                    client_instance: "a".into(),
                    wrapper: "w2".into(),
                    resource: "r2".into(),
                    mode: WrapperApprovalMode::UnlockedUntil { until_epoch: 42 },
                    writable: true,
                    reason: None,
                },
                AgentGrant {
                    client_instance: "b".into(),
                    wrapper: "w3".into(),
                    resource: "r3".into(),
                    mode: WrapperApprovalMode::PerCallApproval {
                        approval_key: "ssh-ed25519 AAAA".into(),
                    },
                    writable: false,
                    reason: None,
                },
            ],
        };
        let json = serde_json::to_string(&doc).unwrap();
        let parsed: PolicyDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.grants, doc.grants);
    }
}
