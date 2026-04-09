//! Policy evaluation types.
//!
//! Phase 1: policy is derived entirely from the config — a wrapper may only
//! read the secret referenced in its WrapperDef.  Future phases add unlock
//! state, write permissions, and richer authorization.

/// Result of a policy check.
#[derive(Debug, Clone)]
pub enum PolicyDecision {
    Allow,
    Deny { reason: String },
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
}
