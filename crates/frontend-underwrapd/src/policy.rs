//! Wrapper-permission gate the daemon consults before serving a wrapper.
//!
//! A wrapper permission is keyed on `(client_instance, wrapper, resource)`.
//! Each wrapper projects exactly one resource, so the daemon consults
//! exactly one permission per invocation:
//!
//! - missing permission   → denied (no implicit allow);
//! - `Locked` (or expired `UnlockedUntil`) → denied;
//! - `PerCallApproval` → invocation needs an approval round before the
//!   serve proceeds;
//! - otherwise the invocation proceeds and the daemon serves the
//!   wrapper's resource.
//!
//! An absent policy document is treated identically to an empty one
//! (deny-by-default). The backend's wrapper-register handler seeds a
//! `Locked` permission for every new `(client_instance, wrapper,
//! resource)` pair, so a freshly-bootstrapped store also denies until
//! an operator unlocks each pair explicitly.

use anyhow::{Context, Result};
use underwrap_protocol::policy::{
    POLICY_DOCUMENT_SECRET_ID, PolicyDecision, PolicyDocument, WrapperPermission,
};
use underwrap_store::StorageBackend;

pub(crate) fn load_policy_document(store: &dyn StorageBackend) -> Result<Option<PolicyDocument>> {
    let Some(material) = store.read(POLICY_DOCUMENT_SECRET_ID)? else {
        return Ok(None);
    };
    let document: PolicyDocument =
        serde_json::from_slice(material.as_ref()).context("parsing backend policy document")?;
    Ok(Some(document))
}

/// Combined decision for one wrapper invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InvocationDecision {
    /// The resource is allowed; the daemon may serve the read directly.
    Allow,
    /// The resource is gated by `PerCallApproval`. The caller must run
    /// an approval round against `approval_key`; on success the read
    /// proceeds.
    NeedsApproval { approval_key: String },
    /// The resource denies the invocation.
    Deny { reason: String },
}

/// Evaluate the wrapper permission for `(client_instance, wrapper, resource)`.
pub(crate) fn evaluate_invocation(
    document: Option<&PolicyDocument>,
    client_instance: &str,
    wrapper: &str,
    resource: &str,
) -> InvocationDecision {
    // Deny-by-default: an absent policy document is treated like an
    // empty one. The backend's wrapper-register handler seeds a
    // `Locked` permission for every new `(client_instance, wrapper,
    // resource)` pair, so a missing document at this layer always
    // means "no permission has been granted yet".
    let Some(doc) = document else {
        return InvocationDecision::Deny {
            reason: format!("no permission for ({client_instance}, '{wrapper}', '{resource}')"),
        };
    };

    let Some(permission) = doc.find_permission(client_instance, wrapper, resource) else {
        return InvocationDecision::Deny {
            reason: format!("no permission for ({client_instance}, '{wrapper}', '{resource}')"),
        };
    };

    match doc.evaluate_permission(client_instance, wrapper, resource) {
        Some(PolicyDecision::Allow) => InvocationDecision::Allow,
        Some(PolicyDecision::Deny { reason }) => InvocationDecision::Deny { reason },
        Some(PolicyDecision::NeedsApproval { approval_key }) => {
            InvocationDecision::NeedsApproval { approval_key }
        }
        // `evaluate_permission` returns `None` only when the permission
        // disappears between `find_permission` and the re-lookup. Treat
        // it the same as a missing permission.
        None => InvocationDecision::Deny {
            reason: format!(
                "permission for ({}, '{wrapper}', '{}') vanished mid-evaluation",
                permission.client_instance, permission.resource_id,
            ),
        },
    }
}

/// After the caller has satisfied a per-call approval round, the
/// previously `NeedsApproval` permission is treated as `Allow`. We
/// re-verify the permission still exists (a concurrent admin could have
/// deleted it between the approval round and the serve).
pub(crate) fn evaluate_invocation_after_approval(
    document: Option<&PolicyDocument>,
    client_instance: &str,
    wrapper: &str,
    resource: &str,
) -> InvocationDecision {
    let revoked = || InvocationDecision::Deny {
        reason: format!(
            "permission for ({client_instance}, '{wrapper}', '{resource}') was revoked"
        ),
    };
    let Some(doc) = document else {
        return revoked();
    };
    if doc
        .find_permission(client_instance, wrapper, resource)
        .is_none()
    {
        return revoked();
    }
    InvocationDecision::Allow
}

/// Whether the wrapper may write through the resource channel for
/// `(client_instance, wrapper, resource)`. Returns `false` when no
/// permission exists or when the policy document itself is absent —
/// writes follow the same deny-by-default rule as reads.
pub(crate) fn writable_for(
    document: Option<&PolicyDocument>,
    client_instance: &str,
    wrapper: &str,
    resource: &str,
) -> bool {
    document
        .and_then(|doc| doc.find_permission(client_instance, wrapper, resource))
        .map(|p: &WrapperPermission| p.writable)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use underwrap_protocol::policy::WrapperApprovalMode;

    fn permission(
        client_instance: &str,
        wrapper: &str,
        resource: &str,
        mode: WrapperApprovalMode,
    ) -> WrapperPermission {
        WrapperPermission {
            client_instance: client_instance.into(),
            wrapper: wrapper.into(),
            resource_id: resource.into(),
            mode,
            writable: false,
            reason: None,
        }
    }

    #[test]
    fn no_document_denies_by_default() {
        match evaluate_invocation(None, "dev", "gh", "github/token") {
            InvocationDecision::Deny { reason } => assert!(reason.contains("no permission")),
            other => panic!("expected Deny, got {other:?}"),
        }
        assert!(!writable_for(None, "dev", "gh", "github/token"));
    }

    #[test]
    fn no_document_after_approval_denies() {
        match evaluate_invocation_after_approval(None, "dev", "gh", "github/token") {
            InvocationDecision::Deny { reason } => assert!(reason.contains("revoked")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn empty_document_denies_with_missing_permission() {
        let doc = PolicyDocument::default();
        assert!(matches!(
            evaluate_invocation(Some(&doc), "dev", "gh", "github/token"),
            InvocationDecision::Deny { .. },
        ));
    }

    #[test]
    fn missing_permission_denies() {
        let doc = PolicyDocument {
            permissions: vec![permission(
                "other",
                "gh",
                "github/token",
                WrapperApprovalMode::Unlocked,
            )],
        };
        match evaluate_invocation(Some(&doc), "dev", "gh", "github/token") {
            InvocationDecision::Deny { reason } => assert!(reason.contains("no permission")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn locked_permission_denies() {
        let doc = PolicyDocument {
            permissions: vec![permission(
                "dev",
                "gh",
                "gws/state",
                WrapperApprovalMode::Locked,
            )],
        };
        assert!(matches!(
            evaluate_invocation(Some(&doc), "dev", "gh", "gws/state"),
            InvocationDecision::Deny { .. }
        ));
    }

    #[test]
    fn per_call_approval_surfaces_key() {
        let doc = PolicyDocument {
            permissions: vec![permission(
                "dev",
                "gh",
                "github/token",
                WrapperApprovalMode::PerCallApproval {
                    approval_key: "ssh-ed25519 AAAA".into(),
                },
            )],
        };
        match evaluate_invocation(Some(&doc), "dev", "gh", "github/token") {
            InvocationDecision::NeedsApproval { approval_key } => {
                assert_eq!(approval_key, "ssh-ed25519 AAAA");
            }
            other => panic!("expected NeedsApproval, got {other:?}"),
        }
    }

    #[test]
    fn writable_defaults_to_false_with_doc_but_no_permission() {
        let doc = PolicyDocument {
            permissions: vec![permission(
                "other",
                "gh",
                "github/token",
                WrapperApprovalMode::Unlocked,
            )],
        };
        assert!(!writable_for(Some(&doc), "dev", "gh", "github/token"));
    }

    #[test]
    fn writable_honours_per_permission_flag() {
        let mut doc = PolicyDocument {
            permissions: vec![permission(
                "dev",
                "gh",
                "github/token",
                WrapperApprovalMode::Unlocked,
            )],
        };
        assert!(!writable_for(Some(&doc), "dev", "gh", "github/token"));
        doc.permissions[0].writable = true;
        assert!(writable_for(Some(&doc), "dev", "gh", "github/token"));
    }
}
