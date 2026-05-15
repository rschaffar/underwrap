# Task: CSRF token, origin check, and logout

Parent: `210_story_rust_native_auth_sessions_multiuser.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- On every authenticated response, set a `__Host-uw-csrf` cookie whose value is a random 32-byte token derived from the session's `csrf_secret`. Not `HttpOnly` (the SPA reads it via JS).
- All non-GET/HEAD/OPTIONS `/v1/admin/...` requests must carry `X-CSRF-Token` matching the cookie, plus an `Origin` (falling back to `Referer`) in the configured allowlist from ticket 200.
- `POST /v1/admin/auth/logout`:
  - Clears the session and CSRF cookies.
  - Adds the session id to a server-side revocation set with TTL = original session expiry.
  - Future requests with the revoked cookie return 401.
- Failures return `403` with stable JSON error codes: `csrf_required`, `csrf_mismatch`, `bad_origin`, `session_revoked`.

## Key files likely involved

- `crates/backend-backendd/src/auth.rs`
- `crates/backend-backendd/src/server.rs`

## Acceptance criteria

- [x] Every state-changing request without matching CSRF token rejected with 403.
- [x] Cross-origin POST rejected with 403.
- [x] Logout invalidates session; reused old cookie returns 401 even before its original expiry.
- [x] Tests cover: missing token, mismatched token, missing origin, allowed origin via Referer fallback, logout invalidation, that GETs are not blocked by CSRF, that revocation TTL eventually frees the revocation set entry.

## Implementation notes

- CSRF cookie `__Host-uw-csrf` is set by both the login response and
  every subsequent session-authenticated response. Its value is the
  session's `csrf_secret`. The SPA echoes it back in `X-CSRF-Token`.
- `check_csrf` verifies the header is non-empty, matches the cookie
  (double-submit), and matches the session's `csrf_secret` (binds to
  the session — guards against substitution).
- CSRF is enforced only for session-cookie auth. Bearer-token callers
  (scripts/CLI) aren't subject to drive-by cross-site requests.
- `SessionPayload` gains a random 128-bit `session_id` so logout can
  name the session in the revocation set. Old (pre-212) cookies
  without `session_id` simply fail deserialization and force a
  re-login — acceptable in greenfield.
- `SessionRevocations` stores `session_id → expires_at`. Entries are
  treated as "not revoked" once `now > expires_at` because the cookie
  itself would fail `decrypt_session` past that point; opportunistic
  cleanup keeps the map bounded.
- Origin/Referer check was already in place from ticket 200; this
  ticket just confirms the `bad_origin` error code for cross-origin
  POSTs.
- Logout itself is CSRF-protected: an attacker on another origin
  cannot log a user out without already possessing the CSRF token.
