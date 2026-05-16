//! Grant-mode gate the daemon consults before serving a wrapper.
//!
//! Pre-245 the daemon evaluated a per-`(wrapper, resource, operation)`
//! policy on every read. After the grant-model rewrite the gate is a
//! single lookup of `(wrapper, host, user)` in the per-store policy
//! document:
//!
//! - No grant → reject. Same shape as the old "ungranted" path.
//! - Grant exists → consult `mode`:
//!   - `Locked`              → deny
//!   - `Unlocked`            → allow
//!   - `UnlockedUntil { … }` → allow if `now < until`, else deny
//!   - `PerCallApproval`     → run the existing ssh-agent challenge
//!
//! Once the mode says allow (directly, or via a satisfied approval
//! round), the daemon serves whatever resources the wrapper's
//! declaration lists. There is no per-resource read filter.

use anyhow::{Context, Result};
use underwrap_protocol::policy::{POLICY_DOCUMENT_SECRET_ID, PolicyDecision, PolicyDocument};
use underwrap_store::StorageBackend;

pub(crate) fn load_policy_document(store: &dyn StorageBackend) -> Result<Option<PolicyDocument>> {
    let Some(material) = store.read(POLICY_DOCUMENT_SECRET_ID)? else {
        return Ok(None);
    };
    let document: PolicyDocument =
        serde_json::from_slice(material.as_ref()).context("parsing backend policy document")?;
    Ok(Some(document))
}

/// Evaluate the grant gate. `None` (returned wrapped in `Ok`) means
/// no grant exists for this principal — the caller surfaces that as a
/// reject. The bare-`PolicyDocument`-absent case (fresh store, no
/// document persisted yet) is treated as "everyone allowed" so the
/// out-of-the-box experience matches a single-user local install.
pub(crate) fn authorize_grant(
    store: &dyn StorageBackend,
    wrapper: &str,
    host: &str,
    user: &str,
) -> Result<Option<PolicyDecision>> {
    let document = load_policy_document(store)?;
    Ok(evaluate_grant_with(&document, wrapper, host, user))
}

pub(crate) fn evaluate_grant_with(
    document: &Option<PolicyDocument>,
    wrapper: &str,
    host: &str,
    user: &str,
) -> Option<PolicyDecision> {
    match document {
        Some(document) => document.evaluate_grant(wrapper, host, user),
        // No document persisted yet = fresh store. The greenfield
        // first-launch experience leaves the daemon permissive; the
        // operator creates a policy document via the admin UI to lock
        // down access.
        None => Some(PolicyDecision::Allow),
    }
}

pub(crate) fn evaluate_grant_after_approval_with(
    document: &Option<PolicyDocument>,
    wrapper: &str,
    host: &str,
    user: &str,
) -> Option<PolicyDecision> {
    match document {
        Some(document) => document.evaluate_grant_after_approval(wrapper, host, user),
        None => Some(PolicyDecision::Allow),
    }
}

/// Whether wrapper-side writes to `<resource>/files/*` are permitted.
/// `None` document → fresh-store/permissive (writes allowed); same
/// "no document → permissive" invariant we use for the grant gate so
/// the out-of-the-box experience matches a single-user local install.
pub(crate) fn files_writable_for(document: &Option<PolicyDocument>, resource: &str) -> bool {
    match document {
        Some(doc) => doc.files_writable_for(resource),
        None => true,
    }
}

/// Same as [`files_writable_for`] for the libsecret slot.
pub(crate) fn libsecret_writable_for(document: &Option<PolicyDocument>, resource: &str) -> bool {
    match document {
        Some(doc) => doc.libsecret_writable_for(resource),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MemStore;
    use underwrap_protocol::policy::{AgentGrant, WrapperApprovalMode};

    fn doc_with(grant: AgentGrant) -> PolicyDocument {
        PolicyDocument {
            grants: vec![grant],
            ..Default::default()
        }
    }

    #[test]
    fn no_policy_document_is_permissive() {
        let store = MemStore::with_policy(None);
        assert_eq!(
            authorize_grant(&store, "gh-comment", "deepthought", "robert")
                .unwrap()
                .unwrap(),
            PolicyDecision::Allow,
        );
    }

    #[test]
    fn missing_grant_returns_none() {
        let store = MemStore::with_policy(Some(PolicyDocument::default()));
        assert!(
            authorize_grant(&store, "gh-comment", "deepthought", "robert")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn locked_grant_denies() {
        let store = MemStore::with_policy(Some(doc_with(AgentGrant {
            wrapper: "gh-comment".into(),
            host: "deepthought".into(),
            user: "robert".into(),
            mode: WrapperApprovalMode::Locked,
            reason: Some("locked centrally".into()),
        })));
        assert!(matches!(
            authorize_grant(&store, "gh-comment", "deepthought", "robert")
                .unwrap()
                .unwrap(),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn per_call_approval_surfaces_needs_approval() {
        let store = MemStore::with_policy(Some(doc_with(AgentGrant {
            wrapper: "aws-prod".into(),
            host: "deepthought".into(),
            user: "robert".into(),
            mode: WrapperApprovalMode::PerCallApproval {
                approval_key: "ssh-ed25519 AAAA".into(),
            },
            reason: None,
        })));
        match authorize_grant(&store, "aws-prod", "deepthought", "robert")
            .unwrap()
            .unwrap()
        {
            PolicyDecision::NeedsApproval { approval_key } => {
                assert!(approval_key.starts_with("ssh-ed25519"));
            }
            other => panic!("expected NeedsApproval, got {other:?}"),
        }
    }

    #[test]
    fn unlocked_grant_allows() {
        let store = MemStore::with_policy(Some(doc_with(AgentGrant {
            wrapper: "gh-comment".into(),
            host: "deepthought".into(),
            user: "robert".into(),
            mode: WrapperApprovalMode::Unlocked,
            reason: None,
        })));
        assert_eq!(
            authorize_grant(&store, "gh-comment", "deepthought", "robert")
                .unwrap()
                .unwrap(),
            PolicyDecision::Allow,
        );
    }
}
