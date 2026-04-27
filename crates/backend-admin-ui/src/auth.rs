//! Operator authentication, session cookies, and CSRF helpers.

use anyhow::{Result, anyhow};
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use base64::Engine;
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;

/// Session cookie name.
pub const SESSION_COOKIE: &str = "underwrap_admin_session";

/// Default session lifetime — 8 hours.
pub const SESSION_LIFETIME_SECONDS: u64 = 8 * 3600;

/// Verify `password` against a stored Argon2 PHC hash.
pub fn verify_password(password: &str, phc_hash: &str) -> Result<()> {
    let parsed =
        PasswordHash::new(phc_hash).map_err(|e| anyhow!("parsing stored password hash: {e}"))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| anyhow!("invalid password"))
}

/// A decoded session cookie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub operator: String,
    pub issued_at_epoch: u64,
    pub expires_at_epoch: u64,
    /// Per-session token used for CSRF protection on state-changing forms.
    pub csrf_token: String,
}

impl Session {
    /// Issue a new session for `operator`, valid for the default lifetime.
    pub fn new(operator: impl Into<String>) -> Self {
        let now = now_epoch();
        Self {
            operator: operator.into(),
            issued_at_epoch: now,
            expires_at_epoch: now + SESSION_LIFETIME_SECONDS,
            csrf_token: random_b64(24),
        }
    }

    pub fn is_expired(&self) -> bool {
        now_epoch() >= self.expires_at_epoch
    }

    /// Encode as `<operator>.<issued>.<expires>.<csrf>.<hmac>` and base64 the
    /// whole thing.
    pub fn encode(&self, key: &[u8; 32]) -> String {
        let body = format!(
            "{}.{}.{}.{}",
            url_escape(&self.operator),
            self.issued_at_epoch,
            self.expires_at_epoch,
            url_escape(&self.csrf_token),
        );
        let mac = hmac_sha256(key, body.as_bytes());
        let mac_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac);
        format!("{body}.{mac_b64}")
    }

    /// Decode the cookie value, verifying the HMAC and expiry.
    pub fn decode(key: &[u8; 32], value: &str) -> Result<Self> {
        let parts: Vec<&str> = value.split('.').collect();
        anyhow::ensure!(parts.len() == 5, "malformed session cookie");
        let body = format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3]);
        let expected_mac = hmac_sha256(key, body.as_bytes());
        let given_mac = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[4])
            .map_err(|_| anyhow!("session cookie mac not base64"))?;
        anyhow::ensure!(
            given_mac.len() == expected_mac.len() && given_mac.ct_eq(&expected_mac).into(),
            "session cookie mac mismatch"
        );
        let operator = url_unescape(parts[0])?;
        let issued_at_epoch: u64 = parts[1].parse().map_err(|_| anyhow!("bad issued_at"))?;
        let expires_at_epoch: u64 = parts[2].parse().map_err(|_| anyhow!("bad expires_at"))?;
        let csrf_token = url_unescape(parts[3])?;
        let session = Self {
            operator,
            issued_at_epoch,
            expires_at_epoch,
            csrf_token,
        };
        anyhow::ensure!(!session.is_expired(), "session expired");
        Ok(session)
    }
}

/// Constant-time compare of two CSRF token strings.
pub fn csrf_token_matches(expected: &str, given: &str) -> bool {
    expected.len() == given.len() && expected.as_bytes().ct_eq(given.as_bytes()).into()
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn random_b64(len_bytes: usize) -> String {
    let mut buf = vec![0u8; len_bytes];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&buf)
}

fn url_escape(s: &str) -> String {
    // Restrict the encoded form to base64url characters by substituting any
    // '.' or '%' we may encounter. Operators are ASCII-safe in practice but
    // this keeps the format strictly unambiguous.
    s.chars()
        .map(|c| match c {
            '.' | '%' => format!("%{:02X}", c as u32),
            c => c.to_string(),
        })
        .collect()
}

fn url_unescape(s: &str) -> Result<String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hi = chars.next().ok_or_else(|| anyhow!("truncated % escape"))?;
            let lo = chars.next().ok_or_else(|| anyhow!("truncated % escape"))?;
            let byte = u8::from_str_radix(&format!("{hi}{lo}"), 16)
                .map_err(|_| anyhow!("bad hex escape"))?;
            out.push(byte as char);
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

/// Minimal HMAC-SHA256 using only the `sha2` crate... but we don't have it.
/// Use a small self-contained implementation based on the `ring` crate would
/// require another dep. Instead, roll a tiny HMAC over SHA-256 using the
/// `argon2` crate's underlying `blake` primitives — but those aren't exposed.
///
/// For simplicity pull in a tiny SHA-256 implementation via the `subtle`-less
/// route by using `ring`. But ring is heavy. The lightest option is the
/// `hmac` + `sha2` crate pair.
///
/// *Actually*: we already pull in `rustls`'s ring indirectly across the
/// workspace. Reaching for `hmac`+`sha2` here is the standard move and
/// weighs ~20 KB. This module adds those as workspace deps in Cargo.toml.
fn hmac_sha256(key: &[u8; 32], data: &[u8]) -> [u8; 32] {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    let result = mac.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::{PasswordHasher, password_hash::SaltString};

    fn hash_password(password: &str) -> String {
        let salt = SaltString::encode_b64(b"abcdefghijklmnopabcdefghijklmnop").unwrap();
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .unwrap()
            .to_string()
    }

    #[test]
    fn verify_password_accepts_correct_and_rejects_wrong() {
        let hash = hash_password("hunter2");
        verify_password("hunter2", &hash).unwrap();
        assert!(verify_password("wrong", &hash).is_err());
    }

    #[test]
    fn session_roundtrips_through_encode_decode() {
        let key = [42u8; 32];
        let session = Session::new("alice");
        let encoded = session.encode(&key);
        let decoded = Session::decode(&key, &encoded).unwrap();
        assert_eq!(decoded.operator, "alice");
        assert_eq!(decoded.csrf_token, session.csrf_token);
    }

    #[test]
    fn session_decode_rejects_tampered_value() {
        let key = [42u8; 32];
        let session = Session::new("alice");
        let encoded = session.encode(&key);
        let tampered = encoded.replacen("alice", "bob", 1);
        assert!(Session::decode(&key, &tampered).is_err());
    }

    #[test]
    fn session_decode_rejects_wrong_key() {
        let session = Session::new("alice");
        let encoded = session.encode(&[1u8; 32]);
        assert!(Session::decode(&[2u8; 32], &encoded).is_err());
    }

    #[test]
    fn session_decode_rejects_expired() {
        let mut session = Session::new("alice");
        session.expires_at_epoch = 1;
        let encoded = session.encode(&[42u8; 32]);
        assert!(Session::decode(&[42u8; 32], &encoded).is_err());
    }

    #[test]
    fn csrf_token_matches_rejects_different_values() {
        assert!(csrf_token_matches("abc", "abc"));
        assert!(!csrf_token_matches("abc", "abd"));
        assert!(!csrf_token_matches("abc", "abcd"));
    }
}
