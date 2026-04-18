//! Approval state management.
//!
//! Tracks per-wrapper, per-scope, per-uid approval grants with time-based expiry.
//! Scope is a caller-provided convenience label; the uid binding ensures that
//! one user's approval cannot be reused by another process with a different uid.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Key for an approval grant: (wrapper_name, scope, uid).
/// scope=None means "all scopes" for this wrapper+uid.
type GrantKey = (String, Option<String>, u32);

/// An active approval grant.
#[derive(Debug, Clone)]
struct Grant {
    /// Expiry time. None = no expiry (approved until explicit revoke).
    expires_at: Option<Instant>,
}

impl Grant {
    fn is_valid(&self) -> bool {
        match self.expires_at {
            Some(t) => Instant::now() < t,
            None => true,
        }
    }
}

/// Pending challenge for SSH agent approval.
#[derive(Debug)]
struct PendingChallenge {
    nonce: Vec<u8>,
    expires_at: Instant,
}

/// Thread-safe approval state store.
pub struct ApprovalState {
    grants: Mutex<HashMap<GrantKey, Grant>>,
    /// Pending SSH agent challenges: (wrapper, scope, uid) → nonce.
    pending: Mutex<HashMap<GrantKey, PendingChallenge>>,
}

impl Default for ApprovalState {
    fn default() -> Self {
        Self::new()
    }
}

impl ApprovalState {
    pub fn new() -> Self {
        Self {
            grants: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Create an approval grant for a wrapper, optionally scoped, bound to a uid.
    pub fn approve(&self, wrapper: &str, scope: Option<&str>, seconds: Option<u64>, uid: u32) {
        let key = (wrapper.to_string(), scope.map(|s| s.to_string()), uid);
        let grant = Grant {
            expires_at: seconds.map(|s| Instant::now() + std::time::Duration::from_secs(s)),
        };
        let mut grants = self.grants.lock().unwrap();
        grants.insert(key, grant);
    }

    /// Lock a specific wrapper (optionally scoped), or everything.
    pub fn revoke(&self, wrapper: Option<&str>, scope: Option<&str>) {
        let mut grants = self.grants.lock().unwrap();
        match wrapper {
            Some(w) => match scope {
                Some(s) => {
                    // Remove all grants for this wrapper+scope regardless of uid.
                    grants.retain(|k, _| !(k.0 == w && k.1.as_deref() == Some(s)));
                }
                None => {
                    grants.retain(|k, _| k.0 != w);
                }
            },
            None => {
                grants.clear();
            }
        }
    }

    /// Check if a wrapper is approved for the given scope and uid.
    pub fn check(&self, wrapper: &str, scope: Option<&str>, uid: u32) -> Result<(), String> {
        let mut grants = self.grants.lock().unwrap();

        // Clean up expired grants.
        grants.retain(|_, g| g.is_valid());

        let w = wrapper.to_string();
        let s = scope.map(|s| s.to_string());

        // Try exact match: (wrapper, scope, uid)
        if let Some(g) = grants.get(&(w.clone(), s.clone(), uid))
            && g.is_valid()
        {
            return Ok(());
        }

        // Try global scope: (wrapper, None, uid)
        if let Some(g) = grants.get(&(w.clone(), None, uid))
            && g.is_valid()
        {
            return Ok(());
        }

        Err(format!("wrapper '{wrapper}' is not approved"))
    }

    /// Store a pending challenge nonce for SSH agent approval.
    /// Expires after 60 seconds.
    pub fn set_pending_challenge(
        &self,
        wrapper: &str,
        scope: Option<&str>,
        uid: u32,
        nonce: &[u8],
    ) {
        let key = (wrapper.to_string(), scope.map(|s| s.to_string()), uid);
        let mut pending = self.pending.lock().unwrap();
        pending.retain(|_, c| Instant::now() < c.expires_at);
        pending.insert(
            key,
            PendingChallenge {
                nonce: nonce.to_vec(),
                expires_at: Instant::now() + std::time::Duration::from_secs(60),
            },
        );
    }

    /// Verify and consume a pending challenge nonce.
    pub fn verify_pending_challenge(
        &self,
        wrapper: &str,
        scope: Option<&str>,
        uid: u32,
        nonce: &[u8],
    ) -> bool {
        let key = (wrapper.to_string(), scope.map(|s| s.to_string()), uid);
        let mut pending = self.pending.lock().unwrap();
        if let Some(challenge) = pending.remove(&key) {
            Instant::now() < challenge.expires_at && challenge.nonce == nonce
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approve_and_check() {
        let state = ApprovalState::new();
        state.approve("w", None, None, 1000);
        assert!(state.check("w", None, 1000).is_ok());
    }

    #[test]
    fn locked_by_default() {
        let state = ApprovalState::new();
        assert!(state.check("w", None, 1000).is_err());
    }

    #[test]
    fn time_limited_still_valid() {
        let state = ApprovalState::new();
        state.approve("w", None, Some(3600), 1000);
        assert!(state.check("w", None, 1000).is_ok());
    }

    #[test]
    fn time_limited_expired() {
        let state = ApprovalState::new();
        state.approve("w", None, Some(0), 1000);
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(state.check("w", None, 1000).is_err());
    }

    #[test]
    fn scoped_approval() {
        let state = ApprovalState::new();
        state.approve("w", Some("s1"), None, 1000);
        assert!(state.check("w", Some("s1"), 1000).is_ok());
        assert!(state.check("w", Some("s2"), 1000).is_err());
    }

    #[test]
    fn uid_isolation() {
        let state = ApprovalState::new();
        state.approve("w", None, None, 1000);
        assert!(state.check("w", None, 1000).is_ok());
        assert!(state.check("w", None, 2000).is_err());
    }

    #[test]
    fn global_scope_covers_scoped_check() {
        let state = ApprovalState::new();
        state.approve("w", None, None, 1000);
        assert!(state.check("w", Some("any-scope"), 1000).is_ok());
    }

    #[test]
    fn revoke_specific() {
        let state = ApprovalState::new();
        state.approve("w1", None, None, 1000);
        state.approve("w2", None, None, 1000);
        state.revoke(Some("w1"), None);
        assert!(state.check("w1", None, 1000).is_err());
        assert!(state.check("w2", None, 1000).is_ok());
    }

    #[test]
    fn revoke_everything() {
        let state = ApprovalState::new();
        state.approve("w1", None, None, 1000);
        state.approve("w2", None, None, 1000);
        state.revoke(None, None);
        assert!(state.check("w1", None, 1000).is_err());
        assert!(state.check("w2", None, 1000).is_err());
    }

    #[test]
    fn lock_scoped() {
        let state = ApprovalState::new();
        state.approve("w", Some("s1"), None, 1000);
        state.approve("w", Some("s2"), None, 1000);
        state.revoke(Some("w"), Some("s1"));
        assert!(state.check("w", Some("s1"), 1000).is_err());
        assert!(state.check("w", Some("s2"), 1000).is_ok());
    }

    #[test]
    fn multiple_reads_same_grant() {
        let state = ApprovalState::new();
        state.approve("w", None, Some(3600), 1000);
        assert!(state.check("w", None, 1000).is_ok());
        assert!(state.check("w", None, 1000).is_ok());
    }
}
