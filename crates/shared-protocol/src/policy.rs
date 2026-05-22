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
//! - a [`WrapperApprovalMode`] (`Locked`, `Unlocked`, `UnlockedUntil`)
//!   deciding *if* the permission is usable right now;
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
    UnlockedUntil {
        #[ts(type = "number")]
        until_epoch: u64,
    },
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

    /// Evaluate a permission tuple at a given wall-clock time. Returns
    /// `None` when no matching permission exists, `Some(Allow)` when
    /// the caller may proceed, or `Some(Deny)` otherwise.
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
    }
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
