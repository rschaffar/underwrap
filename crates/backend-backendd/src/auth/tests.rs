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
