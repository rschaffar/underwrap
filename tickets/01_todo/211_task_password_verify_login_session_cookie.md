# Task: Argon2id login and signed session cookie

Parent: `210_story_rust_native_auth_sessions_multiuser.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Add the `argon2` crate; verify operator passwords against PHC-encoded Argon2id hashes.
- Reuse the existing 32-byte session key file (currently consumed by the Next.js app) as the AEAD key for session cookies. Move config to `AdminConfig.session_key_file`.
- Encrypt session payload with ChaCha20-Poly1305 (`chacha20poly1305` crate) — payload contains `(operator_id, issued_at, expiry, csrf_secret, random_nonce)`.
- Cookie attributes:
  - Name: `__Host-uw-session`
  - `HttpOnly`
  - `SameSite=Strict`
  - `Secure` when TLS is configured (refuse to issue without `Secure` unless listen is loopback)
  - `Path=/`
  - `Max-Age` matches session lifetime (default 12h, configurable)
- `POST /v1/admin/auth/login`:
  - Rate-limit per-username (in-memory token bucket, default 5 attempts per minute per username).
  - Constant-time username lookup to avoid a timing oracle on user existence.
- Audit every login attempt (success/failure) with the operator name and a redacted client IP. Never log the password.

## Key files likely involved

- `crates/backend-backendd/src/server.rs`
- New `crates/backend-backendd/src/auth.rs`
- `crates/backend-backendd/Cargo.toml` (add `argon2`, `chacha20poly1305`, `cookie`)

## Acceptance criteria

- [ ] Argon2id verify uses OWASP-2025-recommended parameters (or stronger).
- [ ] Session cookie round-trips encrypt → decrypt; tamper detection via AEAD tag.
- [ ] Rate limiting blocks brute force after the configurable threshold.
- [ ] Username enumeration timing oracle mitigated: a timing-stability test asserts mean response time for unknown-user is within a tolerance of known-user.
- [ ] Tests cover: success, wrong password, unknown user, expired cookie, tampered cookie, replay of an issued-then-logged-out cookie, refusal to issue without `Secure` when not on loopback, audit-emission for success and failure.
