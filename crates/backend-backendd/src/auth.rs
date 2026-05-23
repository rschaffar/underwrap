//! Operator authentication backend, password verification, and login
//! rate limiting.
//!
//! Session management — cookie issuance, expiry, server-side store —
//! is handled by [`tower_sessions`]. Per-request user resolution
//! happens via [`axum_login::AuthSession`], which calls this module's
//! [`AuthBackend`] to look up users by id (and to authenticate
//! credentials on login). CSRF protection is provided by `axum_csrf`.
//!
//! What lives here:
//!
//! - [`AuthBackend`] implements [`AuthnBackend`] against the operator
//!   collection on disk. `authenticate` runs the per-username rate
//!   limit, then Argon2id verify against the matching operator (or
//!   against [`DUMMY_PASSWORD_HASH`] when no operator matches — the
//!   constant-time enumeration defence).
//! - [`AuthUser`] is implemented for [`UserRecord`] so that
//!   `axum_login` can serialize the user id and the password-hash bytes
//!   into the session. When the hash changes (password rotation) or
//!   the user is deleted, `axum_login` invalidates every existing
//!   session for that operator on the next request — that is how the
//!   server-side revocation guarantee is realised.
//! - [`RateLimiter`] tracks per-username login attempts.
//! - [`verify_password`] / [`hash_password`] are thin Argon2id helpers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordVerifier};
use axum_login::{AuthUser, AuthnBackend, UserId};

use crate::users::{self, UserRecord, normalise_username};

/// Default session lifetime when [`crate::config::AdminConfig::session_lifetime_secs`]
/// is unset: 12 hours. The `tower-sessions` cookie uses this as its
/// `Max-Age` and the server-side store uses it as inactivity expiry.
pub const DEFAULT_SESSION_LIFETIME_SECS: u64 = 12 * 60 * 60;

/// Fixed Argon2id hash used as a constant-time dummy when no operator
/// matches the presented username. Real verify cost dominates timing,
/// so doing this work for the unknown-user path closes the enumeration
/// oracle. The hash itself is meaningless (no real password produces
/// it).
pub const DUMMY_PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$Y29uc3RhbnQtdGltZS1zYWx0$\
     ZGVmaW5pdGVseS1ub3QtYS1yZWFsLWhhc2gtdmFsdWVfcGFkX3BhZF8";

/// Maximum login attempts allowed per username per [`LOGIN_RATE_LIMIT_WINDOW`].
pub const LOGIN_RATE_LIMIT_ATTEMPTS: u32 = 5;
pub const LOGIN_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);

impl AuthUser for UserRecord {
    type Id = String;

    fn id(&self) -> Self::Id {
        self.id.clone()
    }

    /// Returning the password-hash bytes binds every session to a
    /// specific password state: `axum_login` records this on login and
    /// re-checks it on every subsequent request, so rotating an
    /// operator's password (or deleting the operator) invalidates
    /// every existing session for that operator without any explicit
    /// store walk.
    fn session_auth_hash(&self) -> &[u8] {
        self.password_hash.as_bytes()
    }
}

/// Credentials submitted to [`AuthBackend::authenticate`].
#[derive(Debug, Clone)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    #[error("login rate limit exceeded")]
    RateLimited,
    #[error("user collection unavailable: {0}")]
    UserCollection(String),
}

/// `axum_login` backend backed by the operator collection file plus a
/// per-username login rate limiter. Cloning shares the rate-limiter
/// state across all clones.
#[derive(Clone)]
pub struct AuthBackend {
    users_dir: PathBuf,
    rate_limiter: Arc<RateLimiter>,
}

impl AuthBackend {
    pub fn new(users_dir: PathBuf) -> Self {
        Self {
            users_dir,
            rate_limiter: Arc::new(RateLimiter::new()),
        }
    }

    /// Access the shared rate limiter, e.g. for tests that need to
    /// drive its state directly.
    pub fn rate_limiter(&self) -> &RateLimiter {
        &self.rate_limiter
    }
}

impl AuthnBackend for AuthBackend {
    type User = UserRecord;
    type Credentials = Credentials;
    type Error = AuthError;

    async fn authenticate(
        &self,
        creds: Credentials,
    ) -> std::result::Result<Option<UserRecord>, AuthError> {
        let username_norm = normalise_username(&creds.username);
        // Empty username / password short-circuits before touching the
        // rate limiter so that an attacker can't lock out a real
        // account by spamming empty payloads.
        if username_norm.is_empty() || creds.password.is_empty() {
            return Ok(None);
        }
        if !self.rate_limiter.check_and_record(&username_norm) {
            return Err(AuthError::RateLimited);
        }
        let collection =
            users::load(&self.users_dir).map_err(|e| AuthError::UserCollection(e.to_string()))?;
        let lookup = collection.find_by_username(&username_norm);
        // Always run Argon2id verify, even on unknown user, so the
        // response timing does not reveal user existence.
        let hash_to_check = lookup
            .map(|u| u.password_hash.as_str())
            .unwrap_or(DUMMY_PASSWORD_HASH);
        let password_ok = verify_password(hash_to_check, &creds.password);
        match (lookup, password_ok) {
            (Some(u), true) => {
                self.rate_limiter.reset(&username_norm);
                Ok(Some(u.clone()))
            }
            _ => Ok(None),
        }
    }

    async fn get_user(
        &self,
        user_id: &UserId<Self>,
    ) -> std::result::Result<Option<UserRecord>, AuthError> {
        let collection =
            users::load(&self.users_dir).map_err(|e| AuthError::UserCollection(e.to_string()))?;
        Ok(collection.find_by_id(user_id).cloned())
    }
}

/// Verify a password against an Argon2id PHC hash. Returns `Ok(true)`
/// on match. Argon2 verify is constant-time with respect to the hash
/// (we still always run it on the unknown-user path via
/// [`DUMMY_PASSWORD_HASH`] to close the enumeration timing oracle).
pub fn verify_password(hash: &str, password: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(p) => p,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Minimum length operator passwords must satisfy at the entry points
/// that accept a raw password (bootstrap, password change, user create,
/// user reset-password). Login does not consult this rule — verifying
/// against a real hash leaks no length information either way, and
/// rejecting by length there would expose the rule to an unauthenticated
/// probe.
pub const MIN_PASSWORD_LEN: usize = 12;

/// Validate an operator-supplied password before hashing. Returns
/// `Ok(())` on accept, or a stable code suitable for an `AdminError`
/// envelope (`password_empty` / `password_too_short`).
pub fn validate_password(password: &str) -> std::result::Result<(), &'static str> {
    if password.is_empty() {
        return Err("password_empty");
    }
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err("password_too_short");
    }
    Ok(())
}

/// Hash a password with Argon2id and the OWASP-recommended params for
/// the `argon2` crate's defaults.
pub fn hash_password(password: &str) -> Result<String> {
    use argon2::password_hash::{PasswordHasher, SaltString};
    use rand::rngs::OsRng;
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("argon2 hash failed: {e}"))?;
    Ok(hash.to_string())
}

/// Per-username login attempt counter. Resets when the window expires.
#[derive(Default, Debug)]
pub struct RateLimiter {
    inner: Mutex<HashMap<String, (u32, Instant)>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if the attempt is allowed; false if the caller is
    /// over the budget for the current window. The username is
    /// normalised to lower case before tracking.
    pub fn check_and_record(&self, username: &str) -> bool {
        self.check_and_record_with(username, Instant::now())
    }

    fn check_and_record_with(&self, username: &str, now: Instant) -> bool {
        let key = username.trim().to_lowercase();
        let mut guard = self.inner.lock().unwrap();
        let entry = guard.entry(key).or_insert((0, now));
        if now.duration_since(entry.1) >= LOGIN_RATE_LIMIT_WINDOW {
            *entry = (0, now);
        }
        if entry.0 >= LOGIN_RATE_LIMIT_ATTEMPTS {
            return false;
        }
        entry.0 += 1;
        true
    }

    /// Drop the counter for a username after a successful login so the
    /// operator isn't penalised for typos in the past minute.
    pub fn reset(&self, username: &str) {
        let key = username.trim().to_lowercase();
        self.inner.lock().unwrap().remove(&key);
    }
}

#[cfg(test)]
mod tests;
