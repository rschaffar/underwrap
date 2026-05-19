//! Grant-mode gate the daemon consults before serving a wrapper.
//!
//! A grant is keyed on `(client_instance, wrapper, resource)`. For each
//! wrapper invocation the daemon evaluates one grant per resource the
//! wrapper declares and combines them:
//!
//! - any `Locked` (or expired `UnlockedUntil`) → whole invocation denied;
//! - any missing grant → denied (no implicit allow);
//! - any `PerCallApproval` → invocation needs an approval round; all
//!   per-call-approval modes for one invocation must share the same
//!   `approval_key`, otherwise the daemon denies (a single ssh-agent
//!   signature can't satisfy two different keys at once);
//! - otherwise the invocation proceeds and the daemon serves every
//!   declared resource.

use anyhow::{Context, Result};
use underwrap_protocol::policy::{
    AgentGrant, POLICY_DOCUMENT_SECRET_ID, PolicyDecision, PolicyDocument,
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

/// Combined decision for a wrapper invocation that may touch multiple resources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InvocationDecision {
    /// Every resource is allowed; the daemon may serve the read directly.
    Allow,
    /// At least one resource is gated by `PerCallApproval`. The caller
    /// must run a single approval round against `approval_key`; on
    /// success the read proceeds.
    NeedsApproval { approval_key: String },
    /// At least one resource denies the invocation.
    Deny { reason: String },
}

/// Evaluate the grants for `(client_instance, wrapper, resource_i)` over
/// every resource the wrapper declares.
pub(crate) fn evaluate_invocation(
    document: Option<&PolicyDocument>,
    client_instance: &str,
    wrapper: &str,
    resources: &[String],
) -> InvocationDecision {
    // No document persisted yet — fresh store, greenfield permissive
    // default so a new install works out of the box. An *explicit*
    // empty document (operator wrote one with `grants: []`) is a
    // commitment to manage access and is strict — every resource
    // must have its own grant before reads succeed.
    let Some(doc) = document else {
        return InvocationDecision::Allow;
    };

    let mut approval_key: Option<String> = None;
    for resource in resources {
        let Some(grant) = doc.find_grant(client_instance, wrapper, resource) else {
            return InvocationDecision::Deny {
                reason: format!("no grant for ({client_instance}, '{wrapper}', '{resource}')"),
            };
        };
        match doc.evaluate_grant(client_instance, wrapper, resource) {
            Some(PolicyDecision::Allow) => {}
            Some(PolicyDecision::Deny { reason }) => {
                return InvocationDecision::Deny { reason };
            }
            Some(PolicyDecision::NeedsApproval { approval_key: key }) => {
                if let Some(existing) = &approval_key {
                    if existing != &key {
                        return InvocationDecision::Deny {
                            reason: format!(
                                "wrapper '{wrapper}' mixes per-call-approval keys across resources; \
                                 set all per-call grants for one wrapper to the same approval_key"
                            ),
                        };
                    }
                } else {
                    approval_key = Some(key);
                }
            }
            // `evaluate_grant` returns `None` only when the grant
            // disappears between `find_grant` and the second
            // re-lookup; the caller has already filtered for the
            // grant's existence so it is logically unreachable, but
            // we treat it the same as a missing grant.
            None => {
                return InvocationDecision::Deny {
                    reason: format!(
                        "grant for ({}, '{wrapper}', '{}') vanished mid-evaluation",
                        grant.client_instance, grant.resource,
                    ),
                };
            }
        }
    }

    match approval_key {
        Some(approval_key) => InvocationDecision::NeedsApproval { approval_key },
        None => InvocationDecision::Allow,
    }
}

/// After the caller has satisfied a per-call approval round, every
/// grant that was previously `NeedsApproval` is treated as `Allow`. We
/// only need to re-verify that all grants still exist (a concurrent
/// admin could have deleted one between the approval round and the
/// second pass).
pub(crate) fn evaluate_invocation_after_approval(
    document: Option<&PolicyDocument>,
    client_instance: &str,
    wrapper: &str,
    resources: &[String],
) -> InvocationDecision {
    let Some(doc) = document else {
        return InvocationDecision::Allow;
    };
    for resource in resources {
        if doc.find_grant(client_instance, wrapper, resource).is_none() {
            return InvocationDecision::Deny {
                reason: format!(
                    "grant for ({client_instance}, '{wrapper}', '{resource}') was revoked"
                ),
            };
        }
    }
    InvocationDecision::Allow
}

/// Whether the wrapper may write through the secretsd channel for
/// `(client_instance, wrapper, resource)`. Returns `false` when no
/// grant exists; defaults to `true` only when the whole policy document
/// is absent (fresh store, greenfield permissive default).
pub(crate) fn writable_for(
    document: Option<&PolicyDocument>,
    client_instance: &str,
    wrapper: &str,
    resource: &str,
) -> bool {
    match document {
        // No document persisted — greenfield permissive default.
        None => true,
        Some(doc) => doc
            .find_grant(client_instance, wrapper, resource)
            .map(|g: &AgentGrant| g.writable)
            .unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use underwrap_protocol::policy::WrapperApprovalMode;

    fn grant(
        client_instance: &str,
        wrapper: &str,
        resource: &str,
        mode: WrapperApprovalMode,
    ) -> AgentGrant {
        AgentGrant {
            client_instance: client_instance.into(),
            wrapper: wrapper.into(),
            resource: resource.into(),
            mode,
            writable: false,
            reason: None,
        }
    }

    #[test]
    fn no_document_is_permissive() {
        assert_eq!(
            evaluate_invocation(None, "dev", "gh", &["github/token".into()]),
            InvocationDecision::Allow,
        );
    }

    #[test]
    fn empty_document_denies_with_missing_grant() {
        let doc = PolicyDocument::default();
        assert!(matches!(
            evaluate_invocation(Some(&doc), "dev", "gh", &["github/token".into()]),
            InvocationDecision::Deny { .. },
        ));
    }

    #[test]
    fn missing_grant_denies() {
        let doc = PolicyDocument {
            grants: vec![grant(
                "other",
                "gh",
                "github/token",
                WrapperApprovalMode::Unlocked,
            )],
        };
        match evaluate_invocation(Some(&doc), "dev", "gh", &["github/token".into()]) {
            InvocationDecision::Deny { reason } => assert!(reason.contains("no grant")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn locked_grant_denies_whole_invocation() {
        let doc = PolicyDocument {
            grants: vec![
                grant("dev", "gh", "github/token", WrapperApprovalMode::Unlocked),
                grant("dev", "gh", "gws/state", WrapperApprovalMode::Locked),
            ],
        };
        assert!(matches!(
            evaluate_invocation(
                Some(&doc),
                "dev",
                "gh",
                &["github/token".into(), "gws/state".into()],
            ),
            InvocationDecision::Deny { .. }
        ));
    }

    #[test]
    fn per_call_approval_surfaces_one_key() {
        let doc = PolicyDocument {
            grants: vec![
                grant(
                    "dev",
                    "gh",
                    "github/token",
                    WrapperApprovalMode::PerCallApproval {
                        approval_key: "ssh-ed25519 AAAA".into(),
                    },
                ),
                grant(
                    "dev",
                    "gh",
                    "gws/state",
                    WrapperApprovalMode::PerCallApproval {
                        approval_key: "ssh-ed25519 AAAA".into(),
                    },
                ),
            ],
        };
        match evaluate_invocation(
            Some(&doc),
            "dev",
            "gh",
            &["github/token".into(), "gws/state".into()],
        ) {
            InvocationDecision::NeedsApproval { approval_key } => {
                assert_eq!(approval_key, "ssh-ed25519 AAAA");
            }
            other => panic!("expected NeedsApproval, got {other:?}"),
        }
    }

    #[test]
    fn mixed_approval_keys_deny() {
        let doc = PolicyDocument {
            grants: vec![
                grant(
                    "dev",
                    "gh",
                    "github/token",
                    WrapperApprovalMode::PerCallApproval {
                        approval_key: "ssh-ed25519 A".into(),
                    },
                ),
                grant(
                    "dev",
                    "gh",
                    "gws/state",
                    WrapperApprovalMode::PerCallApproval {
                        approval_key: "ssh-ed25519 B".into(),
                    },
                ),
            ],
        };
        assert!(matches!(
            evaluate_invocation(
                Some(&doc),
                "dev",
                "gh",
                &["github/token".into(), "gws/state".into()],
            ),
            InvocationDecision::Deny { .. }
        ));
    }

    #[test]
    fn writable_defaults_to_false_with_doc_but_no_grant() {
        let doc = PolicyDocument {
            grants: vec![grant(
                "other",
                "gh",
                "github/token",
                WrapperApprovalMode::Unlocked,
            )],
        };
        assert!(!writable_for(Some(&doc), "dev", "gh", "github/token"));
    }

    #[test]
    fn writable_honours_per_grant_flag() {
        let mut doc = PolicyDocument {
            grants: vec![grant(
                "dev",
                "gh",
                "github/token",
                WrapperApprovalMode::Unlocked,
            )],
        };
        assert!(!writable_for(Some(&doc), "dev", "gh", "github/token"));
        doc.grants[0].writable = true;
        assert!(writable_for(Some(&doc), "dev", "gh", "github/token"));
    }
}
