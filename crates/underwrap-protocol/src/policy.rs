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
