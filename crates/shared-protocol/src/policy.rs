//! Central policy model shared by backend and frontend.
//!
//! A wrapper permission is keyed on `(client_instance, wrapper, resource)`
//! — the only client-side identifier the backend actually authenticates is
//! the mTLS-bound runtime client instance id. Everything else the daemon
//! would have self-reported (host, local user, instance name from the
//! YAML config) is gone.
//!
//! Each permission carries:
//!
//! - a [`WrapperApprovalMode`] (`Locked`, `Unlocked`,
//!   `UnlockedUntil`, `PerCallApproval`) deciding *if* the permission is
//!   usable right now;
//! - a `writable` flag controlling wrapper-side writes through the
//!   secretsd channel for the `files` and `libsecret` slots (`env`
//!   has no write path);
//! - an optional operator-facing `reason` surfaced in the wrapper
//!   permissions UI.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Reserved secret id used to persist the policy document inside a backend store.
pub const POLICY_DOCUMENT_SECRET_ID: &str = "__underwrap/policy/document";

/// Result of checking a permission's mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// The permission is active; the wrapper may operate.
    Allow,
    /// The permission denies this invocation.
    Deny { reason: String },
    /// The permission requires a fresh ssh-agent signature over a backend-issued
    /// nonce, verified against `approval_key`.
    NeedsApproval { approval_key: String },
}

/// Runtime mode for one [`WrapperPermission`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[derive(Default)]
pub enum WrapperApprovalMode {
    /// Deny every invocation through this permission.
    Locked,
    /// Allow every invocation through this permission.
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

/// One per-store wrapper permission.
///
/// The tuple `(client_instance, wrapper, resource_id)` is the unique
/// key; persistence layers MUST reject duplicates with the stable
/// error code `duplicate_permission`. `resource_id` is the stable
/// backend id, so renaming a resource preserves every permission
/// that references it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct WrapperPermission {
    pub client_instance: String,
    pub wrapper: String,
    pub resource_id: String,
    #[serde(default)]
    pub mode: WrapperApprovalMode,
    /// Whether the wrapper may write to this resource through the
    /// secretsd channel. Applies to the `files` and `libsecret` slots;
    /// `env` has no wrapper-side write path. Defaults to `false`.
    #[serde(default)]
    pub writable: bool,
    /// Operator-facing free-form note. Surfaced in the wrapper
    /// permissions UI tooltip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
}

/// Store-backed policy document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, TS)]
#[ts(export)]
pub struct PolicyDocument {
    /// The flat list of wrapper permissions. One per
    /// `(client_instance, wrapper, resource)` tuple.
    #[serde(default)]
    pub permissions: Vec<WrapperPermission>,
}

/// The three secret "slots" a wrapper resource carries. The first two
/// have a wrapper-side write path and honour the per-permission `writable`
/// flag; `Env` is daemon→target only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretSlot {
    Env,
    Files,
    Libsecret,
}

impl PolicyDocument {
    /// Look up the permission for `(client_instance, wrapper, resource_id)`.
    pub fn find_permission(
        &self,
        client_instance: &str,
        wrapper: &str,
        resource_id: &str,
    ) -> Option<&WrapperPermission> {
        self.permissions.iter().find(|p| {
            p.client_instance == client_instance
                && p.wrapper == wrapper
                && p.resource_id == resource_id
        })
    }

    /// Evaluate the daemon's gate for a given permission tuple.
    ///
    /// - `None` → no matching permission exists.
    /// - `Some(Allow)` → caller may proceed.
    /// - `Some(Deny { reason })` → caller is rejected.
    /// - `Some(NeedsApproval { approval_key })` → caller must run the
    ///   per-call ssh-agent challenge flow.
    pub fn evaluate_permission_at(
        &self,
        client_instance: &str,
        wrapper: &str,
        resource_id: &str,
        now_epoch: u64,
    ) -> Option<PolicyDecision> {
        let permission = self.find_permission(client_instance, wrapper, resource_id)?;
        Some(decide(permission, now_epoch))
    }

    pub fn evaluate_permission(
        &self,
        client_instance: &str,
        wrapper: &str,
        resource_id: &str,
    ) -> Option<PolicyDecision> {
        self.evaluate_permission_at(client_instance, wrapper, resource_id, now_epoch_secs())
    }

    /// After the caller has satisfied a per-call approval round, the mode
    /// gate is already known to allow; this returns `Allow` for the
    /// existing permission tuple and `None` if the permission was deleted
    /// between the approval round and the second pass.
    pub fn evaluate_permission_after_approval(
        &self,
        client_instance: &str,
        wrapper: &str,
        resource_id: &str,
    ) -> Option<PolicyDecision> {
        self.find_permission(client_instance, wrapper, resource_id)
            .map(|_| PolicyDecision::Allow)
    }

    /// True when the wrapper is permitted to write to `resource_id`
    /// (any slot with a wrapper-side write path). Defaults to `false`
    /// (no permission → not writable).
    pub fn writable_for(&self, client_instance: &str, wrapper: &str, resource_id: &str) -> bool {
        self.find_permission(client_instance, wrapper, resource_id)
            .map(|p| p.writable)
            .unwrap_or(false)
    }
}

fn decide(permission: &WrapperPermission, now_epoch: u64) -> PolicyDecision {
    match permission.mode.clone() {
        WrapperApprovalMode::Locked => PolicyDecision::Deny {
            reason: permission
                .reason
                .clone()
                .unwrap_or_else(|| format!("permission for '{}' is locked", permission.wrapper)),
        },
        WrapperApprovalMode::Unlocked => PolicyDecision::Allow,
        WrapperApprovalMode::UnlockedUntil { until_epoch } => {
            if now_epoch > until_epoch {
                PolicyDecision::Deny {
                    reason: format!(
                        "permission for '{}'/'{}' expired at epoch {until_epoch}, now {now_epoch}",
                        permission.wrapper, permission.resource_id
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

    fn doc_with_permission(mode: WrapperApprovalMode) -> PolicyDocument {
        PolicyDocument {
            permissions: vec![WrapperPermission {
                client_instance: "dev-laptop".to_string(),
                wrapper: "gh-comment".to_string(),
                resource_id: "res_token".to_string(),
                mode,
                writable: false,
                reason: None,
            }],
        }
    }

    #[test]
    fn missing_permission_returns_none() {
        let doc = PolicyDocument::default();
        assert!(doc.evaluate_permission_at("c", "w", "r", 100).is_none());
    }

    #[test]
    fn locked_permission_denies_with_reason() {
        let mut doc = doc_with_permission(WrapperApprovalMode::Locked);
        doc.permissions[0].reason = Some("revoked".into());
        match doc
            .evaluate_permission_at("dev-laptop", "gh-comment", "res_token", 100)
            .unwrap()
        {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "revoked"),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn unlocked_permission_allows() {
        let doc = doc_with_permission(WrapperApprovalMode::Unlocked);
        assert_eq!(
            doc.evaluate_permission_at("dev-laptop", "gh-comment", "res_token", 100)
                .unwrap(),
            PolicyDecision::Allow,
        );
    }

    #[test]
    fn unlocked_until_permission_allows_then_denies_lazily() {
        let doc = doc_with_permission(WrapperApprovalMode::UnlockedUntil { until_epoch: 200 });
        assert_eq!(
            doc.evaluate_permission_at("dev-laptop", "gh-comment", "res_token", 199)
                .unwrap(),
            PolicyDecision::Allow,
        );
        assert!(matches!(
            doc.evaluate_permission_at("dev-laptop", "gh-comment", "res_token", 201)
                .unwrap(),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn per_call_approval_returns_needs_approval() {
        let doc = doc_with_permission(WrapperApprovalMode::PerCallApproval {
            approval_key: "ssh-ed25519 AAAA".into(),
        });
        match doc
            .evaluate_permission_at("dev-laptop", "gh-comment", "res_token", 100)
            .unwrap()
        {
            PolicyDecision::NeedsApproval { approval_key } => {
                assert!(approval_key.starts_with("ssh-ed25519"));
            }
            other => panic!("expected NeedsApproval, got {other:?}"),
        }
    }

    #[test]
    fn evaluate_after_approval_allows_when_permission_still_exists() {
        let doc = doc_with_permission(WrapperApprovalMode::PerCallApproval {
            approval_key: "k".into(),
        });
        assert_eq!(
            doc.evaluate_permission_after_approval("dev-laptop", "gh-comment", "res_token")
                .unwrap(),
            PolicyDecision::Allow,
        );
        let empty = PolicyDocument::default();
        assert!(
            empty
                .evaluate_permission_after_approval("dev-laptop", "gh-comment", "res_token")
                .is_none()
        );
    }

    #[test]
    fn writable_defaults_to_false_without_permission() {
        let doc = PolicyDocument::default();
        assert!(!doc.writable_for("dev-laptop", "gh", "r"));
    }

    #[test]
    fn writable_honours_per_permission_flag() {
        let mut doc = doc_with_permission(WrapperApprovalMode::Unlocked);
        assert!(!doc.writable_for("dev-laptop", "gh-comment", "res_token"));
        doc.permissions[0].writable = true;
        assert!(doc.writable_for("dev-laptop", "gh-comment", "res_token"));
    }

    #[test]
    fn document_roundtrips_through_json_with_each_mode() {
        let doc = PolicyDocument {
            permissions: vec![
                WrapperPermission {
                    client_instance: "a".into(),
                    wrapper: "w1".into(),
                    resource_id: "r1".into(),
                    mode: WrapperApprovalMode::Locked,
                    writable: false,
                    reason: Some("r".into()),
                },
                WrapperPermission {
                    client_instance: "a".into(),
                    wrapper: "w2".into(),
                    resource_id: "r2".into(),
                    mode: WrapperApprovalMode::UnlockedUntil { until_epoch: 42 },
                    writable: true,
                    reason: None,
                },
                WrapperPermission {
                    client_instance: "b".into(),
                    wrapper: "w3".into(),
                    resource_id: "r3".into(),
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
        assert_eq!(parsed.permissions, doc.permissions);
    }
}
