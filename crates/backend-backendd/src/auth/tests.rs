use super::*;

#[test]
fn verify_password_round_trip() {
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

// ---------------- AuthBackend (axum-login integration) ---------------

use axum_login::AuthnBackend;
use std::path::PathBuf;
use tempfile::TempDir;

fn seed_user_file(dir: &std::path::Path, username: &str, password: &str) -> (PathBuf, String) {
    let path = dir.join("users.json");
    let hash = hash_password(password).unwrap();
    let id = crate::users::new_user_id();
    let mut collection = crate::users::UserCollection::default();
    collection
        .add(crate::users::UserRecord {
            id: id.clone(),
            username: username.into(),
            password_hash: hash,
            created_at: 1000,
            password_changed_at: 1000,
        })
        .unwrap();
    crate::users::save(&path, &collection).unwrap();
    (path, id)
}

#[tokio::test]
async fn auth_backend_authenticates_a_real_user() {
    let dir = TempDir::new().unwrap();
    let (users_file, id) = seed_user_file(dir.path(), "alice", "hunter2");
    let backend = AuthBackend::new(users_file);
    let creds = Credentials {
        username: "alice".into(),
        password: "hunter2".into(),
    };
    let user = backend.authenticate(creds).await.unwrap();
    assert!(user.is_some());
    assert_eq!(user.unwrap().id, id);
}

#[tokio::test]
async fn auth_backend_rejects_wrong_password() {
    let dir = TempDir::new().unwrap();
    let (users_file, _) = seed_user_file(dir.path(), "alice", "hunter2");
    let backend = AuthBackend::new(users_file);
    let creds = Credentials {
        username: "alice".into(),
        password: "wrong".into(),
    };
    let result = backend.authenticate(creds).await.unwrap();
    assert!(result.is_none(), "wrong password yields Ok(None)");
}

#[tokio::test]
async fn auth_backend_unknown_user_still_runs_verify() {
    // The constant-time defence: unknown usernames are matched against
    // `DUMMY_PASSWORD_HASH` so the response timing does not reveal
    // user existence. We can't directly observe the verify call here,
    // but we can confirm the path returns `Ok(None)` (rather than
    // erroring on a missing user) which is what the dummy-hash branch
    // produces.
    let dir = TempDir::new().unwrap();
    let (users_file, _) = seed_user_file(dir.path(), "alice", "hunter2");
    let backend = AuthBackend::new(users_file);
    let creds = Credentials {
        username: "ghost".into(),
        password: "anything".into(),
    };
    let result = backend.authenticate(creds).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn auth_backend_rate_limits_repeated_failures() {
    let dir = TempDir::new().unwrap();
    let (users_file, _) = seed_user_file(dir.path(), "alice", "hunter2");
    let backend = AuthBackend::new(users_file);
    for _ in 0..LOGIN_RATE_LIMIT_ATTEMPTS {
        let _ = backend
            .authenticate(Credentials {
                username: "alice".into(),
                password: "wrong".into(),
            })
            .await
            .unwrap();
    }
    let err = backend
        .authenticate(Credentials {
            username: "alice".into(),
            password: "hunter2".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, AuthError::RateLimited));
}

#[tokio::test]
async fn auth_backend_get_user_returns_record_by_id() {
    let dir = TempDir::new().unwrap();
    let (users_file, id) = seed_user_file(dir.path(), "alice", "hunter2");
    let backend = AuthBackend::new(users_file);
    let user = backend.get_user(&id).await.unwrap();
    assert!(user.is_some());
    assert_eq!(user.unwrap().username, "alice");
}

#[tokio::test]
async fn auth_backend_get_user_returns_none_for_deleted() {
    let dir = TempDir::new().unwrap();
    let (users_file, _) = seed_user_file(dir.path(), "alice", "hunter2");
    let backend = AuthBackend::new(users_file);
    let user = backend.get_user(&"no-such-id".to_string()).await.unwrap();
    assert!(user.is_none());
}

/// The whole point of returning the password-hash bytes from
/// `session_auth_hash`: when the hash changes (password rotation) or
/// the operator is deleted, `axum_login`'s session check turns up a
/// mismatch and flushes the session. This test verifies that the
/// bytes axum-login sees DO change when the user's password is
/// rotated on disk — anything else would silently break session
/// revocation.
#[tokio::test]
async fn session_auth_hash_changes_when_password_rotates() {
    use axum_login::AuthUser;
    let dir = TempDir::new().unwrap();
    let (users_file, id) = seed_user_file(dir.path(), "alice", "hunter2");
    let backend = AuthBackend::new(users_file.clone());
    let before = backend
        .get_user(&id)
        .await
        .unwrap()
        .unwrap()
        .session_auth_hash()
        .to_vec();

    let mut collection = crate::users::load(&users_file).unwrap();
    collection
        .update(&id, |u| {
            u.password_hash = hash_password("rotated").unwrap();
        })
        .unwrap();
    crate::users::save(&users_file, &collection).unwrap();

    let after = backend
        .get_user(&id)
        .await
        .unwrap()
        .unwrap()
        .session_auth_hash()
        .to_vec();
    assert_ne!(before, after);
}
