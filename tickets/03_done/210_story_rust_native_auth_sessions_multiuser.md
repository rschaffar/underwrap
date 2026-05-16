# Story: Operator identity, sessions, and CSRF native to backendd

Parent: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Background

Operator passwords currently live in the Next.js app config; sessions are signed by a session key the Node tier holds; the BFF authenticates to the backend with a single shared bearer token. The new design moves all of this into `backend-backendd`:

- Operator records (id, username, Argon2id hash, role, granted_stores) live in the backend's store under a reserved secret id.
- Login produces an authenticated session cookie signed/encrypted by a backendd-held key.
- CSRF + same-origin enforcement happen in Rust.
- The shared bearer token disappears; the browser presents the session cookie directly to `/v1/admin/...`.

## Desired design

- One operator → one Argon2id PHC hash → one role (`admin` or `operator`) → a set of granted store names.
- Sessions: AEAD-encrypted cookie (ChaCha20-Poly1305 with the existing 32-byte session key) containing `(operator_id, issued_at, expiry, csrf_secret, nonce)`.
- CSRF: double-submit. Server sets a `__Host-uw-csrf` cookie (non-`HttpOnly` so JS can read it) and the SPA mirrors it in `X-CSRF-Token` on state-changing requests.
- `Origin` / `Referer` checked against the allowlist set up in ticket 200.
- Bootstrap flow allows first-admin creation when no operator records exist; locks itself after first success. An offline CLI recovery path exists for the locked-out case.

## Tasks

1. `210_task_operator_records_storage_and_migration.md`
2. `211_task_password_verify_login_session_cookie.md`
3. `212_task_csrf_origin_check_and_logout.md`
4. `213_task_role_middleware_and_authz_guards.md`
5. `214_task_bootstrap_first_operator_flow.md`

## Acceptance criteria

- [ ] Operator records stored as a serde DTO under a reserved secret id (e.g. `__underwrap/admin/operators`).
- [ ] `POST /v1/admin/auth/login` accepts `{username, password}`, verifies Argon2id, and issues a signed/encrypted session cookie.
- [ ] `POST /v1/admin/auth/logout` invalidates the session.
- [ ] All `/v1/admin/...` endpoints (except `/auth/login`, `/auth/bootstrap`, and SPA assets) require a valid session.
- [ ] State-changing endpoints require a valid CSRF token plus an allowed `Origin`/`Referer`.
- [ ] Bootstrap creates the first admin when no operator records exist and refuses afterward.
- [ ] Role middleware: admin-only endpoints reject operator-role sessions with 403; store-scoped endpoints filter by `granted_stores`.
- [ ] Negative tests cover: bad password, tampered cookie, expired cookie, missing CSRF token, cross-origin POST, replay of an old session, double-bootstrap, ungranted-store leak.
