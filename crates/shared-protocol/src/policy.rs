//! Central policy model shared by backend and frontend.
//!
//! Ticket 245 simplified this to the bare minimum:
//!
//! - One [`AgentGrant`] row per `(wrapper, host, user)` tuple — the unit of
//!   "agent X on host H running as user U can talk to the daemon for wrapper
//!   W". The grant carries a [`WrapperApprovalMode`] (the same four-state mode
//!   the wrapper UI exposed before), which decides *if* the grant is usable
//!   right now.
//! - One global per-store toggle [`PolicyDocument::writes_enabled`] gates
//!   every write attempt across the store. There is no per-secret write ACL.
//! - Read access through an active grant is **unconditional**. Once the
//!   mode check passes, the daemon hands the wrapper every secret listed in
//!   its declaration — no per-resource filter, no per-secret toggling.
//!
//! The previous per-secret rule/grant ACL and per-secret unlock-until
//! windows are gone. The four `WrapperApprovalMode` variants (`Locked`,
//! `Unlocked`, `UnlockedUntil { until }`, `PerCallApproval { approval_key }`)
//! are unchanged in behaviour and now live on each grant rather than
//! per-wrapper.

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

/// Runtime mode for one [`AgentGrant`]. Same four variants the wrapper-mode
/// UI exposed pre-245; they now live per grant rather than per wrapper.
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
/// The tuple `(wrapper, host, user)` is the unique key; persistence layers
/// MUST reject duplicates with the stable error code `duplicate_grant`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AgentGrant {
    pub wrapper: String,
    pub host: String,
    pub user: String,
    #[serde(default)]
    pub mode: WrapperApprovalMode,
    /// Operator-facing free-form note. Surfaced in the matrix UI tooltip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
}

/// Store-backed policy document for one underwrap instance namespace.
///
/// `serde` accepts extra fields by default, so pre-245 policy blobs
/// (carrying the legacy `wrappers` / `rules` arrays) deserialise into the
/// new shape with those fields silently dropped. The
/// `old_policy_blob_with_legacy_fields_still_parses` regression test pins
/// that contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, TS)]
#[ts(export)]
pub struct PolicyDocument {
    /// The flat list of agent grants. One per `(wrapper, host, user)` tuple.
    #[serde(default)]
    pub grants: Vec<AgentGrant>,
    /// Master kill switch for every write path in this store. `false` by
    /// default — admins opt in deliberately before any mutation lands.
    #[serde(default)]
    pub writes_enabled: bool,
}

impl PolicyDocument {
    /// Look up the grant for `(wrapper, host, user)`.
    pub fn find_grant(&self, wrapper: &str, host: &str, user: &str) -> Option<&AgentGrant> {
        self.grants
            .iter()
            .find(|g| g.wrapper == wrapper && g.host == host && g.user == user)
    }

    /// Evaluate the daemon's gate for a given principal.
    ///
    /// - `None` → no matching grant exists. The caller should respond with
    ///   the existing "ungranted" error path (same as 404 in the admin
    ///   surface).
    /// - `Some(Allow)` → caller may proceed.
    /// - `Some(Deny { reason })` → caller is rejected.
    /// - `Some(NeedsApproval { approval_key })` → caller must run the
    ///   per-call ssh-agent challenge flow.
    pub fn evaluate_grant_at(
        &self,
        wrapper: &str,
        host: &str,
        user: &str,
        now_epoch: u64,
    ) -> Option<PolicyDecision> {
        let grant = self.find_grant(wrapper, host, user)?;
        Some(match grant.mode.clone() {
            WrapperApprovalMode::Locked => PolicyDecision::Deny {
                reason: grant
                    .reason
                    .clone()
                    .unwrap_or_else(|| format!("grant for '{wrapper}' is locked")),
            },
            WrapperApprovalMode::Unlocked => PolicyDecision::Allow,
            WrapperApprovalMode::UnlockedUntil { until_epoch } => {
                if now_epoch > until_epoch {
                    PolicyDecision::Deny {
                        reason: format!(
                            "grant for '{wrapper}' expired at epoch {until_epoch}, now {now_epoch}"
                        ),
                    }
                } else {
                    PolicyDecision::Allow
                }
            }
            WrapperApprovalMode::PerCallApproval { approval_key } => {
                PolicyDecision::NeedsApproval { approval_key }
            }
        })
    }

    pub fn evaluate_grant(&self, wrapper: &str, host: &str, user: &str) -> Option<PolicyDecision> {
        let now_epoch = now_epoch_secs();
        self.evaluate_grant_at(wrapper, host, user, now_epoch)
    }

    /// After the caller has satisfied a per-call approval round, the mode
    /// gate is already known to allow; this returns `Allow` for the
    /// existing grant tuple and `None` if the grant was deleted between
    /// the approval round and the second pass.
    pub fn evaluate_grant_after_approval(
        &self,
        wrapper: &str,
        host: &str,
        user: &str,
    ) -> Option<PolicyDecision> {
        self.find_grant(wrapper, host, user)
            .map(|_| PolicyDecision::Allow)
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
                wrapper: "gh-comment".to_string(),
                host: "deepthought".to_string(),
                user: "robert".to_string(),
                mode,
                reason: None,
            }],
            writes_enabled: false,
        }
    }

    #[test]
    fn missing_grant_returns_none() {
        let doc = PolicyDocument::default();
        assert!(
            doc.evaluate_grant_at("anything", "host", "user", 100)
                .is_none()
        );
    }

    #[test]
    fn locked_grant_denies_with_reason() {
        let mut doc = doc_with_grant(WrapperApprovalMode::Locked);
        doc.grants[0].reason = Some("revoked".into());
        match doc
            .evaluate_grant_at("gh-comment", "deepthought", "robert", 100)
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
            doc.evaluate_grant_at("gh-comment", "deepthought", "robert", 100)
                .unwrap(),
            PolicyDecision::Allow,
        );
    }

    #[test]
    fn unlocked_until_grant_allows_then_denies_lazily() {
        let doc = doc_with_grant(WrapperApprovalMode::UnlockedUntil { until_epoch: 200 });
        assert_eq!(
            doc.evaluate_grant_at("gh-comment", "deepthought", "robert", 199)
                .unwrap(),
            PolicyDecision::Allow,
        );
        assert!(matches!(
            doc.evaluate_grant_at("gh-comment", "deepthought", "robert", 201)
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
            .evaluate_grant_at("gh-comment", "deepthought", "robert", 100)
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
            doc.evaluate_grant_after_approval("gh-comment", "deepthought", "robert")
                .unwrap(),
            PolicyDecision::Allow,
        );
        // ...and returns None if the grant is gone.
        let empty = PolicyDocument::default();
        assert!(
            empty
                .evaluate_grant_after_approval("gh-comment", "deepthought", "robert")
                .is_none()
        );
    }

    #[test]
    fn document_roundtrips_through_json_with_each_mode() {
        let doc = PolicyDocument {
            grants: vec![
                AgentGrant {
                    wrapper: "a".into(),
                    host: "h".into(),
                    user: "u".into(),
                    mode: WrapperApprovalMode::Locked,
                    reason: Some("r".into()),
                },
                AgentGrant {
                    wrapper: "b".into(),
                    host: "h".into(),
                    user: "u".into(),
                    mode: WrapperApprovalMode::UnlockedUntil { until_epoch: 42 },
                    reason: None,
                },
                AgentGrant {
                    wrapper: "c".into(),
                    host: "h".into(),
                    user: "u".into(),
                    mode: WrapperApprovalMode::PerCallApproval {
                        approval_key: "ssh-ed25519 AAAA".into(),
                    },
                    reason: None,
                },
            ],
            writes_enabled: true,
        };
        let json = serde_json::to_string(&doc).unwrap();
        let parsed: PolicyDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.grants, doc.grants);
        assert_eq!(parsed.writes_enabled, doc.writes_enabled);
    }

    #[test]
    fn old_policy_blob_with_legacy_fields_still_parses() {
        // A pre-245 policy document persisted as JSON: contains the
        // dropped `wrappers` and `rules` fields. The new
        // deserialiser must accept it (treat as empty) so an upgrade
        // doesn't refuse to boot against a still-old store blob.
        let legacy = r#"{
            "wrappers": [
                {"wrapper":"old","mode":{"kind":"locked"},"reason":"x"}
            ],
            "rules": [
                {"id":"r","wrapper":"w","resource":"x","operation":"y","effect":"allow"}
            ],
            "grants": []
        }"#;
        let parsed: PolicyDocument = serde_json::from_str(legacy).unwrap();
        assert!(parsed.grants.is_empty());
        assert!(!parsed.writes_enabled);
    }
}
