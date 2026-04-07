//! Unlock state management.
//!
//! Tracks per-wrapper, per-scope, per-uid unlock grants with time-based expiry.
//! Scope is a caller-provided convenience label; the uid binding ensures that
//! one user's unlock cannot be reused by another process with a different uid.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Key for an unlock grant: (wrapper_name, scope, uid).
/// scope=None means "all scopes" for this wrapper+uid.
/// uid=None means "any uid" (admin/root-granted).
type GrantKey = (String, Option<String>, Option<u32>);

/// An active unlock grant.
#[derive(Debug, Clone)]
struct Grant {
    /// Expiry time. None = no expiry (unlocked until explicit lock).
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

/// Pending challenge for SSH agent unlock.
#[derive(Debug)]
struct PendingChallenge {
    nonce_hex: String,
    expires_at: Instant,
}

/// Thread-safe unlock state store.
pub struct UnlockState {
    grants: Mutex<HashMap<GrantKey, Grant>>,
    /// Pending SSH agent challenges: (wrapper, scope, uid) → nonce.
    pending: Mutex<HashMap<GrantKey, PendingChallenge>>,
}

impl Default for UnlockState {
    fn default() -> Self {
        Self::new()
    }
}

impl UnlockState {
    pub fn new() -> Self {
        Self {
            grants: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Create an unlock grant for a wrapper, optionally scoped, bound to a uid.
    pub fn unlock(
        &self,
        wrapper: &str,
        scope: Option<&str>,
        seconds: Option<u64>,
        uid: Option<u32>,
    ) {
        let key = (wrapper.to_string(), scope.map(|s| s.to_string()), uid);
        let grant = Grant {
            expires_at: seconds.map(|s| Instant::now() + std::time::Duration::from_secs(s)),
        };
        let mut grants = self.grants.lock().unwrap();
        grants.insert(key, grant);
    }

    /// Lock a specific wrapper (optionally scoped), or everything.
    pub fn lock(&self, wrapper: Option<&str>, scope: Option<&str>) {
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

    /// Check if a wrapper is unlocked for the given scope and uid.
    pub fn check(
        &self,
        wrapper: &str,
        scope: Option<&str>,
        uid: Option<u32>,
    ) -> Result<(), String> {
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

        // Try uid-wildcard: (wrapper, scope, None) — admin grant
        if let Some(g) = grants.get(&(w.clone(), s.clone(), None))
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

        // Try fully global: (wrapper, None, None)
        if let Some(g) = grants.get(&(w.clone(), None, None))
            && g.is_valid()
        {
            return Ok(());
        }

        Err(format!("wrapper '{wrapper}' is locked"))
    }

    /// Get status info for a wrapper.
    pub fn status(&self, wrapper: &str) -> serde_json::Value {
        let grants = self.grants.lock().unwrap();
        let mut active: Vec<serde_json::Value> = Vec::new();

        for ((w, scope, uid), grant) in grants.iter() {
            if w == wrapper && grant.is_valid() {
                let mut entry = serde_json::json!({
                    "scope": scope,
                    "uid": uid,
                });
                if let Some(expires_at) = grant.expires_at {
                    let remaining = expires_at
                        .checked_duration_since(Instant::now())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    entry["remaining_seconds"] = serde_json::json!(remaining);
                }
                active.push(entry);
            }
        }

        serde_json::json!({
            "wrapper": wrapper,
            "unlocked": !active.is_empty(),
            "grants": active,
        })
    }

    /// Store a pending challenge nonce for SSH agent unlock.
    /// Expires after 60 seconds.
    pub fn set_pending_challenge(
        &self,
        wrapper: &str,
        scope: Option<&str>,
        uid: Option<u32>,
        nonce_hex: &str,
    ) {
        let key = (wrapper.to_string(), scope.map(|s| s.to_string()), uid);
        let mut pending = self.pending.lock().unwrap();
        // Clean up expired challenges.
        pending.retain(|_, c| Instant::now() < c.expires_at);
        pending.insert(
            key,
            PendingChallenge {
                nonce_hex: nonce_hex.to_string(),
                expires_at: Instant::now() + std::time::Duration::from_secs(60),
            },
        );
    }

    /// Verify and consume a pending challenge nonce.
    pub fn verify_pending_challenge(
        &self,
        wrapper: &str,
        scope: Option<&str>,
        uid: Option<u32>,
        nonce_hex: &str,
    ) -> bool {
        let key = (wrapper.to_string(), scope.map(|s| s.to_string()), uid);
        let mut pending = self.pending.lock().unwrap();
        if let Some(challenge) = pending.remove(&key) {
            Instant::now() < challenge.expires_at && challenge.nonce_hex == nonce_hex
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlock_and_check() {
        let state = UnlockState::new();
        state.unlock("w", None, None, Some(1000));
        assert!(state.check("w", None, Some(1000)).is_ok());
    }

    #[test]
    fn locked_by_default() {
        let state = UnlockState::new();
        assert!(state.check("w", None, Some(1000)).is_err());
    }

    #[test]
    fn time_limited_still_valid() {
        let state = UnlockState::new();
        state.unlock("w", None, Some(3600), Some(1000));
        assert!(state.check("w", None, Some(1000)).is_ok());
    }

    #[test]
    fn time_limited_expired() {
        let state = UnlockState::new();
        state.unlock("w", None, Some(0), Some(1000));
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(state.check("w", None, Some(1000)).is_err());
    }

    #[test]
    fn scoped_unlock() {
        let state = UnlockState::new();
        state.unlock("w", Some("s1"), None, Some(1000));
        assert!(state.check("w", Some("s1"), Some(1000)).is_ok());
        assert!(state.check("w", Some("s2"), Some(1000)).is_err());
    }

    #[test]
    fn uid_isolation() {
        let state = UnlockState::new();
        state.unlock("w", None, None, Some(1000));
        // Same uid: allowed.
        assert!(state.check("w", None, Some(1000)).is_ok());
        // Different uid: denied.
        assert!(state.check("w", None, Some(2000)).is_err());
    }

    #[test]
    fn admin_grant_covers_all_uids() {
        let state = UnlockState::new();
        // uid=None is an admin grant.
        state.unlock("w", None, None, None);
        assert!(state.check("w", None, Some(1000)).is_ok());
        assert!(state.check("w", None, Some(2000)).is_ok());
        assert!(state.check("w", None, None).is_ok());
    }

    #[test]
    fn global_scope_covers_scoped_check() {
        let state = UnlockState::new();
        state.unlock("w", None, None, Some(1000));
        assert!(state.check("w", Some("any-scope"), Some(1000)).is_ok());
    }

    #[test]
    fn lock_specific() {
        let state = UnlockState::new();
        state.unlock("w1", None, None, Some(1000));
        state.unlock("w2", None, None, Some(1000));
        state.lock(Some("w1"), None);
        assert!(state.check("w1", None, Some(1000)).is_err());
        assert!(state.check("w2", None, Some(1000)).is_ok());
    }

    #[test]
    fn lock_everything() {
        let state = UnlockState::new();
        state.unlock("w1", None, None, Some(1000));
        state.unlock("w2", None, None, Some(1000));
        state.lock(None, None);
        assert!(state.check("w1", None, Some(1000)).is_err());
        assert!(state.check("w2", None, Some(1000)).is_err());
    }

    #[test]
    fn lock_scoped() {
        let state = UnlockState::new();
        state.unlock("w", Some("s1"), None, Some(1000));
        state.unlock("w", Some("s2"), None, Some(1000));
        state.lock(Some("w"), Some("s1"));
        assert!(state.check("w", Some("s1"), Some(1000)).is_err());
        assert!(state.check("w", Some("s2"), Some(1000)).is_ok());
    }

    #[test]
    fn status_shows_grants() {
        let state = UnlockState::new();
        let s = state.status("w");
        assert_eq!(s["unlocked"], false);

        state.unlock("w", None, Some(3600), Some(1000));
        let s = state.status("w");
        assert_eq!(s["unlocked"], true);
        assert!(s["grants"][0]["remaining_seconds"].as_u64().unwrap() > 0);
    }

    #[test]
    fn multiple_reads_same_grant() {
        let state = UnlockState::new();
        state.unlock("w", None, Some(3600), Some(1000));
        for _ in 0..100 {
            assert!(state.check("w", None, Some(1000)).is_ok());
        }
    }
}
