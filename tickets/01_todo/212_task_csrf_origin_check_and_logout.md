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

- [ ] Every state-changing request without matching CSRF token rejected with 403.
- [ ] Cross-origin POST rejected with 403.
- [ ] Logout invalidates session; reused old cookie returns 401 even before its original expiry.
- [ ] Tests cover: missing token, mismatched token, missing origin, allowed origin via Referer fallback, logout invalidation, that GETs are not blocked by CSRF, that revocation TTL eventually frees the revocation set entry.
