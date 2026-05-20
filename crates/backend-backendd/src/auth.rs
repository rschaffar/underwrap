//! Operator login, session cookies, and password verification.
//!
//! - **Login**: `POST /v1/admin/auth/login` accepts `{username, password}`,
//!   verifies an Argon2id PHC hash from the operator collection, and on
//!   success issues an AEAD-encrypted session cookie.
//! - **Sessions**: the 32-byte session key file (already used by the
//!   Next.js BFF) is repurposed as a ChaCha20-Poly1305 AEAD key. The
//!   payload contains `(operator_id, issued_at, expiry, csrf_secret,
//!   nonce)`. Tampering trips the AEAD tag.
//! - **Rate limiting**: a simple per-username token bucket gates login
//!   attempts so an attacker cannot brute force a single account.
//! - **Constant-time username lookup**: when no operator matches we
//!   still run Argon2id against a fixed dummy hash so the response
//!   timing does not reveal user existence.
//!
//! Ticket 212 builds on this for CSRF + logout; ticket 213 adds role
//! middleware; ticket 214 adds bootstrap. The bearer-token path in
//! `server::authenticate_admin` continues to work in parallel during
//! the transition.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordVerifier};
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};

/// Length of the session AEAD key, in bytes. The session key file must
/// contain exactly this many bytes (the Next.js BFF used the same
/// length).
pub const SESSION_KEY_LEN: usize = 32;

/// Default session lifetime when `AdminConfig.session_lifetime_secs` is
/// unset: 12 hours.
pub const DEFAULT_SESSION_LIFETIME_SECS: u64 = 12 * 60 * 60;

/// Cookie name. `__Host-` prefix forces `Secure`, `Path=/`, and no
/// `Domain`, which neutralises subdomain confusion attacks.
pub const SESSION_COOKIE_NAME: &str = "__Host-uw-session";

/// CSRF cookie name. `__Host-` prefix forces `Secure`, `Path=/`, and no
/// `Domain`. Not `HttpOnly`: the SPA reads it via JS to echo back in
/// `X-CSRF-Token` (double-submit pattern).
pub const CSRF_COOKIE_NAME: &str = "__Host-uw-csrf";

/// Header the SPA sends on state-changing requests, carrying the value
/// it read from the CSRF cookie.
pub const CSRF_HEADER_NAME: &str = "X-CSRF-Token";

/// Fixed Argon2id hash used as a constant-time dummy when no operator
/// matches the presented username. Real verify cost dominates timing,
/// so doing this work for the unknown-user path closes the enumeration
/// oracle. The hash itself is meaningless (no real password produces
/// it).
pub const DUMMY_PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$Y29uc3RhbnQtdGltZS1zYWx0$\
     ZGVmaW5pdGVseS1ub3QtYS1yZWFsLWhhc2gtdmFsdWVfcGFkX3BhZF8";

/// Maximum login attempts allowed per username per window.
pub const LOGIN_RATE_LIMIT_ATTEMPTS: u32 = 5;
pub const LOGIN_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);

/// What we serialise inside the encrypted cookie.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionPayload {
    /// Random 128-bit identifier used by the server-side revocation
    /// set (ticket 212). Distinct from the AEAD nonce: that protects
    /// the cookie ciphertext, this names the session for revocation.
    pub session_id: String,
    pub operator_id: String,
    pub username: String,
    /// Epoch seconds when the session was issued.
    pub issued_at: u64,
    /// Epoch seconds at which the session expires. Checked on decrypt.
    pub expires_at: u64,
    /// 32 random bytes (hex). Doubles as the CSRF cookie value (the
    /// SPA reads the cookie via JS and echoes it back in
    /// `X-CSRF-Token`; the server verifies the header matches this
    /// secret).
    pub csrf_secret: String,
}

/// Errors callers want to distinguish at the HTTP boundary.
#[derive(Debug)]
pub enum SessionError {
    Malformed,
    BadSignature,
    Expired,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed => f.write_str("malformed session cookie"),
            Self::BadSignature => f.write_str("session signature failed (tampered or wrong key)"),
            Self::Expired => f.write_str("session expired"),
        }
    }
}

impl std::error::Error for SessionError {}

/// Load a 32-byte session key from disk. Refuses anything shorter or
/// longer to keep the AEAD construction unambiguous.
pub fn load_session_key(path: &std::path::Path) -> Result<[u8; SESSION_KEY_LEN]> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading session key file {}", path.display()))?;
    if bytes.len() != SESSION_KEY_LEN {
        anyhow::bail!(
            "session key file {} must contain exactly {} bytes (got {})",
            path.display(),
            SESSION_KEY_LEN,
            bytes.len()
        );
    }
    let mut out = [0u8; SESSION_KEY_LEN];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Encrypt a session payload into the cookie value. Returns the
/// `base64url(nonce || ciphertext_and_tag)` string ready to drop into
/// a `Set-Cookie` header.
pub fn encrypt_session(payload: &SessionPayload, key: &[u8; SESSION_KEY_LEN]) -> Result<String> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = serde_json::to_vec(payload).context("serialising session payload to JSON")?;
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_slice())
        .map_err(|e| anyhow::anyhow!("session encrypt failed: {e}"))?;
    let mut combined = Vec::with_capacity(nonce_bytes.len() + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(combined))
}

/// Decrypt and validate a cookie value. Verifies the AEAD tag and the
/// expiry timestamp. `now` is parameterised for tests.
pub fn decrypt_session(
    encoded: &str,
    key: &[u8; SESSION_KEY_LEN],
    now: u64,
) -> std::result::Result<SessionPayload, SessionError> {
    let combined = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded.as_bytes())
        .map_err(|_| SessionError::Malformed)?;
    if combined.len() < 12 + 16 {
        return Err(SessionError::Malformed);
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| SessionError::BadSignature)?;
    let payload: SessionPayload =
        serde_json::from_slice(&plaintext).map_err(|_| SessionError::Malformed)?;
    if payload.expires_at <= now {
        return Err(SessionError::Expired);
    }
    Ok(payload)
}

/// Build a fresh session payload for the given operator.
pub fn new_session_payload(
    operator_id: String,
    username: String,
    now: u64,
    lifetime_secs: u64,
) -> SessionPayload {
    let mut secret = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut secret);
    let mut sid = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut sid);
    SessionPayload {
        session_id: hex::encode(sid),
        operator_id,
        username,
        issued_at: now,
        expires_at: now.saturating_add(lifetime_secs),
        csrf_secret: hex::encode(secret),
    }
}

/// Render a `Set-Cookie` header value for a freshly-encrypted session.
/// `secure` should be true whenever the admin listener has TLS
/// configured (so the cookie's `__Host-` rules are satisfied).
pub fn render_session_cookie(encoded: &str, lifetime_secs: u64, secure: bool) -> String {
    // __Host- requires `Secure; Path=/; no Domain`. We always emit
    // Path=/. `Secure` is conditional only because dev-loopback HTTP
    // would otherwise reject the cookie; production must be TLS.
    let secure_attr = if secure { "Secure; " } else { "" };
    format!(
        "{name}={value}; {secure}HttpOnly; SameSite=Strict; Path=/; Max-Age={age}",
        name = SESSION_COOKIE_NAME,
        value = encoded,
        secure = secure_attr,
        age = lifetime_secs,
    )
}

/// Extract the session cookie value from a `Cookie` header, if present.
pub fn extract_session_cookie(headers: &hyper::HeaderMap) -> Option<&str> {
    extract_cookie(headers, SESSION_COOKIE_NAME)
}

/// Extract the CSRF cookie value from a `Cookie` header, if present.
pub fn extract_csrf_cookie(headers: &hyper::HeaderMap) -> Option<&str> {
    extract_cookie(headers, CSRF_COOKIE_NAME)
}

fn extract_cookie<'a>(headers: &'a hyper::HeaderMap, name: &str) -> Option<&'a str> {
    let header = headers.get(hyper::header::COOKIE)?.to_str().ok()?;
    let prefix = format!("{name}=");
    for part in header.split(';') {
        let trimmed = part.trim();
        if let Some(value) = trimmed.strip_prefix(&prefix) {
            return Some(value);
        }
    }
    None
}

/// Render a `Set-Cookie` value for the CSRF cookie. Mirrors the session
/// cookie's `__Host-` rules but is intentionally NOT `HttpOnly` because
/// the SPA reads it via JS to populate `X-CSRF-Token`.
pub fn render_csrf_cookie(secret: &str, lifetime_secs: u64, secure: bool) -> String {
    let secure_attr = if secure { "Secure; " } else { "" };
    format!(
        "{name}={value}; {secure}SameSite=Strict; Path=/; Max-Age={age}",
        name = CSRF_COOKIE_NAME,
        value = secret,
        secure = secure_attr,
        age = lifetime_secs,
    )
}

/// Render a `Set-Cookie` value that immediately clears the session
/// cookie (Max-Age=0).
pub fn render_session_cookie_clear(secure: bool) -> String {
    let secure_attr = if secure { "Secure; " } else { "" };
    format!(
        "{name}=; {secure}HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        name = SESSION_COOKIE_NAME,
        secure = secure_attr,
    )
}

/// Render a `Set-Cookie` value that immediately clears the CSRF cookie.
pub fn render_csrf_cookie_clear(secure: bool) -> String {
    let secure_attr = if secure { "Secure; " } else { "" };
    format!(
        "{name}=; {secure}SameSite=Strict; Path=/; Max-Age=0",
        name = CSRF_COOKIE_NAME,
        secure = secure_attr,
    )
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

/// Hash a password with Argon2id and the OWASP-recommended params for
/// the `argon2` crate's defaults. Used by future bootstrap / user-CRUD
/// tickets; included here so tests have a way to produce real hashes.
pub fn hash_password(password: &str) -> Result<String> {
    use argon2::password_hash::{PasswordHasher, SaltString};
    use rand::rngs::OsRng;
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("argon2 hash failed: {e}"))?;
    Ok(hash.to_string())
}

/// Server-side set of revoked session ids with their original
/// expiry. A session id stays in the set until `now > expires_at`,
/// after which the underlying cookie can no longer pass
/// [`decrypt_session`] anyway, so the entry is safe to drop.
#[derive(Default, Debug)]
pub struct SessionRevocations {
    inner: Mutex<HashMap<String, u64>>,
    /// Per-username "minimum allowed issued_at" floor. Set when an
    /// admin rotates a user's password or deletes their account so
    /// every session issued before that epoch is invalidated in one
    /// shot — without us having to track which session ids each user
    /// holds. Keyed by `normalise_username`.
    username_floor: Mutex<HashMap<String, u64>>,
}

impl SessionRevocations {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark a session as revoked. Opportunistically drops entries that
    /// have already passed their original expiry to keep the map small.
    pub fn revoke(&self, session_id: String, expires_at: u64, now: u64) {
        let mut guard = self.inner.lock().unwrap();
        guard.retain(|_, exp| *exp > now);
        guard.insert(session_id, expires_at);
    }

    /// Invalidate every session issued for `username` strictly before
    /// `now`. Use when the user's password changes or the user is
    /// deleted, so a still-unexpired cookie can't outlive the change.
    pub fn revoke_username(&self, username: &str, now: u64) {
        let mut guard = self.username_floor.lock().unwrap();
        guard.insert(crate::users::normalise_username(username), now);
    }

    /// Returns true while the session id is in the set AND has not
    /// passed its original expiry. Entries past their expiry are treated
    /// as "not revoked" because the cookie itself would fail expiry
    /// checks on decrypt; this matches the cookie's natural lifetime.
    pub fn is_revoked(&self, session_id: &str, now: u64) -> bool {
        let guard = self.inner.lock().unwrap();
        guard.get(session_id).is_some_and(|exp| *exp > now)
    }

    /// Whether the session payload's `(username, issued_at)` pair
    /// falls below the per-username revocation floor set by
    /// [`Self::revoke_username`]. Distinct from session-id revocation;
    /// callers should treat *either* check returning true as
    /// "session is no longer valid".
    pub fn is_user_floor_revoked(&self, username: &str, issued_at: u64) -> bool {
        let guard = self.username_floor.lock().unwrap();
        guard
            .get(&crate::users::normalise_username(username))
            .is_some_and(|floor| issued_at < *floor)
    }

    /// Drop all entries whose original session expiry has passed.
    pub fn cleanup(&self, now: u64) {
        let mut guard = self.inner.lock().unwrap();
        guard.retain(|_, exp| *exp > now);
    }

    /// Number of entries currently tracked (after cleanup is applied).
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    /// Convenience for tests / health checks.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
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
mod tests {
    use super::*;

    fn fixed_key() -> [u8; SESSION_KEY_LEN] {
        let mut k = [0u8; SESSION_KEY_LEN];
        for (i, b) in k.iter_mut().enumerate() {
            *b = i as u8;
        }
        k
    }

    #[test]
    fn session_round_trip_succeeds() {
        let key = fixed_key();
        let payload = new_session_payload("op-1".into(), "alice".into(), 1000, 3600);
        let cookie = encrypt_session(&payload, &key).unwrap();
        let decoded = decrypt_session(&cookie, &key, 1500).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn session_tamper_is_rejected() {
        let key = fixed_key();
        let payload = new_session_payload("op-1".into(), "alice".into(), 1000, 3600);
        let mut cookie = encrypt_session(&payload, &key).unwrap().into_bytes();
        // Flip a byte in the middle of the ciphertext region.
        let i = cookie.len() / 2;
        cookie[i] ^= 0x01;
        let cookie = String::from_utf8(cookie).unwrap();
        let err = decrypt_session(&cookie, &key, 1500).unwrap_err();
        assert!(matches!(
            err,
            SessionError::BadSignature | SessionError::Malformed
        ));
    }

    #[test]
    fn session_wrong_key_is_rejected() {
        let key = fixed_key();
        let mut other = fixed_key();
        other[0] ^= 1;
        let payload = new_session_payload("op-1".into(), "alice".into(), 1000, 3600);
        let cookie = encrypt_session(&payload, &key).unwrap();
        let err = decrypt_session(&cookie, &other, 1500).unwrap_err();
        assert!(matches!(err, SessionError::BadSignature));
    }

    #[test]
    fn session_expiry_is_enforced() {
        let key = fixed_key();
        let payload = new_session_payload("op-1".into(), "alice".into(), 1000, 60);
        let cookie = encrypt_session(&payload, &key).unwrap();
        let err = decrypt_session(&cookie, &key, 2000).unwrap_err();
        assert!(matches!(err, SessionError::Expired));
    }

    #[test]
    fn malformed_input_is_rejected() {
        let key = fixed_key();
        let err = decrypt_session("not-base64!", &key, 0).unwrap_err();
        assert!(matches!(err, SessionError::Malformed));
        let err = decrypt_session("AAAA", &key, 0).unwrap_err();
        assert!(matches!(err, SessionError::Malformed));
    }

    #[test]
    fn cookie_render_includes_security_attributes() {
        let cookie = render_session_cookie("ciphertext", 600, true);
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("Max-Age=600"));
        assert!(cookie.starts_with("__Host-uw-session="));
    }

    #[test]
    fn cookie_render_omits_secure_when_not_tls() {
        let cookie = render_session_cookie("ciphertext", 600, false);
        assert!(!cookie.contains("Secure"));
    }

    #[test]
    fn extract_session_cookie_parses_combined_header() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert(
            hyper::header::COOKIE,
            hyper::header::HeaderValue::from_static("foo=bar; __Host-uw-session=abc.def; x=y"),
        );
        assert_eq!(extract_session_cookie(&headers), Some("abc.def"));
    }

    #[test]
    fn extract_session_cookie_returns_none_when_missing() {
        let headers = hyper::HeaderMap::new();
        assert!(extract_session_cookie(&headers).is_none());
    }

    #[test]
    fn verify_password_round_trips_with_hash_password() {
        let hash = hash_password("hunter2").unwrap();
        assert!(verify_password(&hash, "hunter2"));
        assert!(!verify_password(&hash, "wrong"));
    }

    #[test]
    fn verify_password_rejects_malformed_hash() {
        assert!(!verify_password("not-a-phc-string", "any"));
    }

    #[test]
    fn dummy_password_hash_never_matches_real_passwords() {
        // It's *possible* but vanishingly unlikely that a random salt
        // happens to produce a hash matching some short password. The
        // dummy hash is hand-crafted and parses as PHC-shaped; verify
        // should consistently return false for typical inputs.
        for candidate in ["", "a", "password", "hunter2", "1234"] {
            assert!(!verify_password(DUMMY_PASSWORD_HASH, candidate));
        }
    }

    #[test]
    fn rate_limiter_blocks_after_threshold() {
        let limiter = RateLimiter::new();
        for _ in 0..LOGIN_RATE_LIMIT_ATTEMPTS {
            assert!(limiter.check_and_record("alice"));
        }
        assert!(!limiter.check_and_record("alice"));
    }

    #[test]
    fn rate_limiter_resets_window_after_expiry() {
        let limiter = RateLimiter::new();
        let t0 = Instant::now();
        for _ in 0..LOGIN_RATE_LIMIT_ATTEMPTS {
            assert!(limiter.check_and_record_with("alice", t0));
        }
        assert!(!limiter.check_and_record_with("alice", t0));
        let t1 = t0 + LOGIN_RATE_LIMIT_WINDOW + Duration::from_secs(1);
        assert!(limiter.check_and_record_with("alice", t1));
    }

    #[test]
    fn rate_limiter_is_case_insensitive() {
        let limiter = RateLimiter::new();
        for _ in 0..LOGIN_RATE_LIMIT_ATTEMPTS {
            limiter.check_and_record("Alice");
        }
        assert!(!limiter.check_and_record("alice"));
    }

    #[test]
    fn rate_limiter_reset_clears_counter() {
        let limiter = RateLimiter::new();
        for _ in 0..LOGIN_RATE_LIMIT_ATTEMPTS {
            limiter.check_and_record("alice");
        }
        limiter.reset("alice");
        assert!(limiter.check_and_record("alice"));
    }

    #[test]
    fn session_payload_includes_session_id() {
        let p1 = new_session_payload("op".into(), "alice".into(), 1000, 3600);
        let p2 = new_session_payload("op".into(), "alice".into(), 1000, 3600);
        assert!(!p1.session_id.is_empty());
        assert_eq!(p1.session_id.len(), 32, "16 bytes hex = 32 chars");
        assert_ne!(p1.session_id, p2.session_id, "ids must be random");
    }

    #[test]
    fn csrf_cookie_render_is_readable_by_js() {
        let cookie = render_csrf_cookie("abc123", 600, true);
        assert!(cookie.starts_with("__Host-uw-csrf=abc123"));
        assert!(cookie.contains("Secure"));
        assert!(
            !cookie.contains("HttpOnly"),
            "CSRF cookie MUST NOT be HttpOnly: the SPA reads it"
        );
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("Max-Age=600"));
    }

    #[test]
    fn csrf_cookie_render_omits_secure_for_plain_http() {
        let cookie = render_csrf_cookie("abc123", 600, false);
        assert!(!cookie.contains("Secure"));
    }

    #[test]
    fn cookie_clear_uses_max_age_zero() {
        let session = render_session_cookie_clear(true);
        assert!(session.contains("__Host-uw-session=;"));
        assert!(session.contains("Max-Age=0"));
        assert!(session.contains("HttpOnly"));
        let csrf = render_csrf_cookie_clear(true);
        assert!(csrf.contains("__Host-uw-csrf=;"));
        assert!(csrf.contains("Max-Age=0"));
        assert!(!csrf.contains("HttpOnly"));
    }

    #[test]
    fn extract_csrf_cookie_parses_combined_header() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert(
            hyper::header::COOKIE,
            hyper::header::HeaderValue::from_static(
                "foo=bar; __Host-uw-session=abc; __Host-uw-csrf=deadbeef; x=y",
            ),
        );
        assert_eq!(extract_csrf_cookie(&headers), Some("deadbeef"));
        assert_eq!(extract_session_cookie(&headers), Some("abc"));
    }

    #[test]
    fn session_revocations_track_until_expiry() {
        let r = SessionRevocations::new();
        assert!(!r.is_revoked("sid", 0));
        r.revoke("sid".into(), 2000, 1000);
        assert!(r.is_revoked("sid", 1500));
        // Past the original session expiry, the entry no longer
        // matters: the cookie itself would fail decrypt_session.
        assert!(!r.is_revoked("sid", 2500));
    }

    #[test]
    fn session_revocations_cleanup_drops_expired_entries() {
        let r = SessionRevocations::new();
        r.revoke("a".into(), 1500, 1000);
        r.revoke("b".into(), 3000, 1000);
        assert_eq!(r.len(), 2);
        r.cleanup(2000);
        assert_eq!(r.len(), 1, "only the still-live entry remains");
        r.cleanup(4000);
        assert!(r.is_empty(), "revocation TTL eventually frees the set");
    }

    #[test]
    fn session_revocations_revoke_opportunistically_cleans() {
        let r = SessionRevocations::new();
        r.revoke("a".into(), 1500, 1000);
        // Inserting a new entry past the first's expiry should drop it.
        r.revoke("b".into(), 5000, 2000);
        assert_eq!(r.len(), 1);
        assert!(!r.is_revoked("a", 2000));
        assert!(r.is_revoked("b", 2000));
    }

    #[test]
    fn load_session_key_rejects_wrong_length() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("key");
        std::fs::write(&p, vec![0u8; 10]).unwrap();
        let err = load_session_key(&p).unwrap_err();
        assert!(err.to_string().contains("32 bytes"));
    }

    #[test]
    fn load_session_key_reads_exact_length() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("key");
        std::fs::write(&p, vec![7u8; SESSION_KEY_LEN]).unwrap();
        let k = load_session_key(&p).unwrap();
        assert_eq!(k, [7u8; SESSION_KEY_LEN]);
    }

    /// Surface-level constant-time check: verify_password against the
    /// dummy hash and against a real hash take similar wall time. This
    /// is a coarse oracle test; CI noise can shift it, so the bound
    /// is generous.
    #[test]
    fn unknown_user_timing_is_within_an_order_of_magnitude() {
        let real_hash = hash_password("hunter2").unwrap();
        let bench = |hash: &str| {
            let start = Instant::now();
            for _ in 0..5 {
                let _ = verify_password(hash, "definitely-wrong");
            }
            start.elapsed()
        };
        let real = bench(&real_hash);
        let dummy = bench(DUMMY_PASSWORD_HASH);
        // The dummy hash should not be radically faster than the real
        // one; same Argon2 params dominate. We accept up to 10×
        // skew to avoid CI flakes.
        let ratio = real.as_nanos() as f64 / dummy.as_nanos().max(1) as f64;
        assert!(
            ratio < 10.0 && ratio > 0.1,
            "verify_password timing ratio between real and dummy hashes is suspicious: {ratio:.2}",
        );
    }
}
