//! Unlock state management.
//!
//! Tracks per-wrapper, per-scope unlock grants with time-based expiry.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Key for an unlock grant: (wrapper_name, scope).
/// scope=None means "all scopes" (global unlock for this wrapper).
type GrantKey = (String, Option<String>);

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

/// Thread-safe unlock state store.
pub struct UnlockState {
    grants: Mutex<HashMap<GrantKey, Grant>>,
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
        }
    }

    /// Create an unlock grant for a wrapper, optionally scoped.
    /// `seconds = None` means unlocked indefinitely (until explicit lock).
    pub fn unlock(&self, wrapper: &str, scope: Option<&str>, seconds: Option<u64>) {
        let key = (wrapper.to_string(), scope.map(|s| s.to_string()));
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
                    grants.remove(&(w.to_string(), Some(s.to_string())));
                }
                None => {
                    // Remove all grants for this wrapper (any scope).
                    grants.retain(|k, _| k.0 != w);
                }
            },
            None => {
                // Lock everything.
                grants.clear();
            }
        }
    }

    /// Check if a wrapper is unlocked for the given scope.
    /// Returns Ok(()) if allowed, Err(reason) if locked.
    pub fn check(&self, wrapper: &str, scope: Option<&str>) -> Result<(), String> {
        let mut grants = self.grants.lock().unwrap();

        // Clean up expired grants.
        grants.retain(|_, g| g.is_valid());

        // Try exact match (wrapper + scope) first.
        let scoped_key = (wrapper.to_string(), scope.map(|s| s.to_string()));
        if let Some(grant) = grants.get(&scoped_key)
            && grant.is_valid()
        {
            return Ok(());
        }

        // Try global (no scope) grant for this wrapper.
        let global_key = (wrapper.to_string(), None);
        if let Some(grant) = grants.get(&global_key)
            && grant.is_valid()
        {
            return Ok(());
        }

        Err(format!("wrapper '{wrapper}' is locked"))
    }

    /// Get status info for a wrapper.
    pub fn status(&self, wrapper: &str) -> serde_json::Value {
        let grants = self.grants.lock().unwrap();
        let mut active: Vec<serde_json::Value> = Vec::new();

        for ((w, scope), grant) in grants.iter() {
            if w == wrapper && grant.is_valid() {
                let mut entry = serde_json::json!({ "scope": scope });
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlock_and_check() {
        let state = UnlockState::new();
        state.unlock("w", None, None);
        assert!(state.check("w", None).is_ok());
    }

    #[test]
    fn locked_by_default() {
        let state = UnlockState::new();
        assert!(state.check("w", None).is_err());
    }

    #[test]
    fn time_limited_still_valid() {
        let state = UnlockState::new();
        state.unlock("w", None, Some(3600));
        assert!(state.check("w", None).is_ok());
    }

    #[test]
    fn time_limited_expired() {
        let state = UnlockState::new();
        state.unlock("w", None, Some(0));
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(state.check("w", None).is_err());
    }

    #[test]
    fn scoped_unlock() {
        let state = UnlockState::new();
        state.unlock("w", Some("session-1"), None);
        assert!(state.check("w", Some("session-1")).is_ok());
        assert!(state.check("w", Some("session-2")).is_err());
        assert!(state.check("w", None).is_err());
    }

    #[test]
    fn global_grant_covers_all_scopes() {
        let state = UnlockState::new();
        state.unlock("w", None, None);
        assert!(state.check("w", Some("any-scope")).is_ok());
        assert!(state.check("w", None).is_ok());
    }

    #[test]
    fn lock_specific() {
        let state = UnlockState::new();
        state.unlock("w1", None, None);
        state.unlock("w2", None, None);
        state.lock(Some("w1"), None);
        assert!(state.check("w1", None).is_err());
        assert!(state.check("w2", None).is_ok());
    }

    #[test]
    fn lock_everything() {
        let state = UnlockState::new();
        state.unlock("w1", None, None);
        state.unlock("w2", None, None);
        state.lock(None, None);
        assert!(state.check("w1", None).is_err());
        assert!(state.check("w2", None).is_err());
    }

    #[test]
    fn lock_scoped() {
        let state = UnlockState::new();
        state.unlock("w", Some("s1"), None);
        state.unlock("w", Some("s2"), None);
        state.lock(Some("w"), Some("s1"));
        assert!(state.check("w", Some("s1")).is_err());
        assert!(state.check("w", Some("s2")).is_ok());
    }

    #[test]
    fn status_shows_grants() {
        let state = UnlockState::new();
        let s = state.status("w");
        assert_eq!(s["unlocked"], false);

        state.unlock("w", None, Some(3600));
        let s = state.status("w");
        assert_eq!(s["unlocked"], true);
        assert!(s["grants"][0]["remaining_seconds"].as_u64().unwrap() > 0);
    }

    #[test]
    fn multiple_reads_same_grant() {
        let state = UnlockState::new();
        state.unlock("w", None, Some(3600));
        for _ in 0..100 {
            assert!(state.check("w", None).is_ok());
        }
    }
}
