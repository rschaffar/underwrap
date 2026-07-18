//! Backend-owned wrapper permission model and shared policy wire types.
//!
//! A permission is keyed on `(client_instance, wrapper, resource)`. The backend
//! authenticates the runtime client instance through mTLS; host names, local
//! users, and frontend-config instance labels are not backend principals.
//!
//! Each permission carries:
//!
//! - a [`WrapperApprovalMode`] (`Locked`, `Unlocked`, `UnlockedUntil`,
//!   `UnsafeDebugUntil`) deciding *if* the permission is usable right
//!   now;
//! - a `writable` flag controlling wrapper-side writes through the
//!   secretsd channel for the `files` and `libsecret` slots (`env`
//!   has no write path);
//! - an optional operator-facing `reason` surfaced in the wrapper
//!   permissions UI.

use serde::{Deserialize, Serialize};

use crate::service::RuntimeDenyReason;
use crate::variant::WrapperVariant;

/// Reserved key used to persist the policy document inside a backend
/// store. Lives under [`crate::control::CONTROL_PREFIX`] so it is never
/// mistaken for user secret material.
pub const POLICY_DOCUMENT_SECRET_ID: &str = "control/policy";

/// Result of checking a permission's mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// The permission is active; the wrapper may operate.
    Allow,
    /// The permission denies this invocation.
    Deny { reason: String },
}

/// Runtime mode for one [`WrapperPermission`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[derive(Default)]
pub enum WrapperApprovalMode {
    /// Deny every invocation through this permission.
    Locked,
    /// Allow real invocations; debug remains denied.
    #[default]
    Unlocked,
    /// Allow invocations until `until_epoch`; deny afterwards. The expiry is
    /// checked lazily on each request — no server-side sweeper.
    UnlockedUntil { until_epoch: u64 },
    /// Allow **both** the real wrapper and its debug variant until
    /// `until_epoch`; deny afterwards. This is the only mode under which a
    /// `variant = Debug` invocation is allowed — the debug tool can drop an
    /// interactive shell or exec arbitrary commands with **read-write**
    /// access to the projected secret (writebacks are accepted on the same
    /// terms as `Real`, gated by `writable`), so it is gated behind an
    /// explicit operator opt-in with a TTL. Expiry is checked lazily per
    /// request.
    UnsafeDebugUntil { until_epoch: u64 },
}

/// One per-store wrapper permission.
///
/// The tuple `(client_instance, wrapper, resource_id)` is the unique
/// key; persistence layers MUST reject duplicates with the stable
/// error code `duplicate_permission`. `resource_id` is the stable
/// backend id, so renaming a resource preserves every permission
/// that references it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
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
    pub reason: Option<String>,
}

/// Store-backed policy document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, utoipa::ToSchema)]
pub struct PolicyDocument {
    /// The flat list of wrapper permissions. One per
    /// `(client_instance, wrapper, resource)` tuple.
    #[serde(default)]
    pub permissions: Vec<WrapperPermission>,
}

/// The three resource slots. `Files` and `Libsecret` have wrapper-side
/// writeback paths governed by the permission's `writable` flag; `Env` is
/// backend-to-target only.
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

    /// Evaluate a permission tuple at a given wall-clock time. Mirrors
    /// the live runtime authorization path (`find_permission` then
    /// [`decide`]) as one call — used by tests; the backend handlers
    /// compose the two directly. Returns `None` when no matching
    /// permission exists, `Some(Allow)` when the caller may proceed, or
    /// `Some(Deny)` otherwise.
    pub fn evaluate_permission_at(
        &self,
        client_instance: &str,
        wrapper: &str,
        resource_id: &str,
        variant: WrapperVariant,
        now_epoch: u64,
    ) -> Option<PolicyDecision> {
        let permission = self.find_permission(client_instance, wrapper, resource_id)?;
        Some(decide(permission, variant, now_epoch))
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

/// Evaluate a permission for a specific invocation `variant`. The
/// matrix (columns = variant):
///
/// | mode                          | Real            | Debug                       |
/// |-------------------------------|-----------------|-----------------------------|
/// | `Locked`                      | Deny            | Deny                        |
/// | `Unlocked`                    | Allow           | Deny (not in unsafe-debug)  |
/// | `UnlockedUntil` not expired   | Allow           | Deny (not in unsafe-debug)  |
/// | `UnlockedUntil` expired       | Deny (expired)  | Deny (expired)              |
/// | `UnsafeDebugUntil` not expired| Allow           | Allow                       |
/// | `UnsafeDebugUntil` expired    | Deny (expired)  | Deny (expired)              |
pub fn decide(
    permission: &WrapperPermission,
    variant: WrapperVariant,
    now_epoch: u64,
) -> PolicyDecision {
    // Deny reasons are the canonical [`RuntimeDenyReason`] discriminators
    // (the single source of truth for these wire strings): `locked`,
    // `expired`, `not-unsafe-debug`. A `Locked` permission with an
    // operator-supplied reason surfaces that reason verbatim instead.
    let expired = || PolicyDecision::Deny {
        reason: RuntimeDenyReason::Expired.to_string(),
    };
    let not_debug_mode = || PolicyDecision::Deny {
        reason: RuntimeDenyReason::NotUnsafeDebug.to_string(),
    };

    match permission.mode.clone() {
        WrapperApprovalMode::Locked => PolicyDecision::Deny {
            reason: permission
                .reason
                .clone()
                .unwrap_or_else(|| RuntimeDenyReason::Locked.to_string()),
        },
        WrapperApprovalMode::Unlocked => match variant {
            WrapperVariant::Real => PolicyDecision::Allow,
            WrapperVariant::Debug => not_debug_mode(),
        },
        WrapperApprovalMode::UnlockedUntil { until_epoch } => {
            if now_epoch > until_epoch {
                expired()
            } else {
                match variant {
                    WrapperVariant::Real => PolicyDecision::Allow,
                    WrapperVariant::Debug => not_debug_mode(),
                }
            }
        }
        WrapperApprovalMode::UnsafeDebugUntil { until_epoch } => {
            if now_epoch > until_epoch {
                expired()
            } else {
                // Both variants are allowed under an active unsafe-debug
                // permission.
                PolicyDecision::Allow
            }
        }
    }
}

#[cfg(test)]
mod tests;
