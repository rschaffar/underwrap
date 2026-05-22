# Task: Migrate sessions to `tower-sessions` + `axum-login`; replace CSRF with `axum_csrf`

Parent: `1700_story_backend_framework_migration.md`
Depends on: `1710_task_port_admin_api_to_axum_router.md`

## Background

After 1710, the admin API runs on Axum but still uses the bespoke
session and CSRF stack:

- `crates/backend-backendd/src/auth.rs` carries the
  ChaCha20-Poly1305 AEAD session cookie (`encrypt_session`,
  `decrypt_session`, `SessionPayload`), the `SessionRevocations`
  map with `session_id` and `username_floor` overlays, the
  session-key loader, `__Host-uw-session` / `__Host-uw-csrf`
  cookie renderers and clearers, and the cookie extractors. ~280
  lines of session/CSRF infrastructure plus ~120 lines of password
  + rate-limiter code.
- `crates/backend-backendd/src/admin_api/csrf.rs` (179 lines)
  carries the three-way CSRF check (header == cookie AND header ==
  `session_payload.csrf_secret`, both constant-time) plus the
  per-response CSRF cookie refresh.

The planning discussion settled on swapping these for the standard
Axum auth stack. The cryptographic content of "compare token to
expected value, constant-time, server-known" is equivalent at
OWASP's current recommended tier; we lose the AEAD-embedded
csrf_secret design but gain a convention every reviewer
recognizes, free revocation/audit, and ~400 lines off the custom
security surface. See `[[project_session_store_decision]]` for
the store choice (in-memory).

## Scope

- Add dependencies to `crates/backend-backendd/Cargo.toml`:
  - `tower-sessions` with the in-memory store feature only.
  - `axum-login`.
  - `axum_csrf` (or equivalent CSRF tower layer that supports
    signed double-submit; pick the one with active maintenance at
    migration time, document the choice in `docs/security.md`).
- Replace session infrastructure in `auth.rs`:
  - Remove `SessionPayload`, `SessionError`, `load_session_key`,
    `encrypt_session`, `decrypt_session`, `new_session_payload`,
    `render_session_cookie`, `render_session_cookie_clear`,
    `render_csrf_cookie`, `render_csrf_cookie_clear`,
    `extract_session_cookie`, `extract_csrf_cookie`,
    `extract_cookie`, `SESSION_KEY_LEN`,
    `DEFAULT_SESSION_LIFETIME_SECS`, `SESSION_COOKIE_NAME`,
    `CSRF_COOKIE_NAME`, `CSRF_HEADER_NAME`, `SessionRevocations`.
  - Keep `verify_password`, `hash_password`, `DUMMY_PASSWORD_HASH`,
    `LOGIN_RATE_LIMIT_*`, and `RateLimiter`. These are
    independent of the session mechanism.
- Drop the session-key file from `AdminConfig`. Remove the
  session-key load step from startup and from
  `runtime_state::AdminRuntimeState`. The NixOS module
  (`nix/modules/underwrap-backend.nix`) stops provisioning a
  session-key file.
- Wire `tower-sessions` with `MemoryStore` configured to:
  - Cookie name `__Host-uw-session`.
  - `Secure` (when TLS configured), `HttpOnly`, `SameSite=Strict`,
    `Path=/`. No `Domain` attribute (required by `__Host-`).
  - Configurable session lifetime defaulting to today's 12 hours
    (`DEFAULT_SESSION_LIFETIME_SECS` was the previous default).
- Wire `axum-login`:
  - Implement `AuthnBackend` against the existing operator
    collection. The backend's `authenticate(credentials)` accepts
    `{username, password}`, runs the existing per-username rate
    limit check, runs `verify_password` against the operator's
    hash (or against `DUMMY_PASSWORD_HASH` if no operator
    matches), and returns the operator on success.
  - Implement `get_user` against the operator collection.
  - Add the `AuthSession` extractor to handler signatures that
    today receive a `Principal` extension. Replace the
    `resolve_principal_layer` with `axum-login`'s built-in
    "require auth" layer, then continue to resolve a `Principal`
    in a small middleware that reads the live operator record
    (preserves the current "rename takes effect immediately"
    behavior — see `admin_api/csrf.rs:121–151`).
- Wire `axum_csrf`:
  - Apply as a tower layer to the protected sub-router. Cookie
    name `__Host-uw-csrf`. `Secure` (TLS), not `HttpOnly`,
    `SameSite=Strict`, `Path=/`.
  - Header name `X-CSRF-Token`.
  - The frontend reads the cookie and echoes it back — unchanged
    semantics from today.
- Replace the in-handler logic at
  `admin_api/auth.rs::handle_admin_login` so that on a successful
  password check it calls `auth_session.login(&user)` instead of
  building an AEAD cookie. Logout calls `auth_session.logout()`.
  The login response continues to carry the same JSON body.
- Replace `admin_api/csrf.rs`:
  - Remove `AdminAuth`, `AdminAuthError`, `authenticate_admin`,
    `check_csrf`, `attach_csrf_cookie`.
  - Keep `resolve_principal` only if it still does work the
    middleware doesn't (live-rename re-read) — otherwise delete
    the file.
- Preserve the per-username rate limiter (`RateLimiter` in
  `auth.rs`) and the constant-time dummy hash. They live inside
  the `AuthnBackend::authenticate` implementation.
- Preserve user-floor revocation behavior. When an admin rotates
  another user's password (`admin_api/users.rs` /
  `admin_api/auth.rs::handle_admin_password_change`) or deletes
  an operator, walk `tower-sessions`'s store and delete every
  session whose user id matches. This is the in-memory-store
  equivalent of `SessionRevocations::revoke_username`.
- Update `docs/security.md` to describe the new session model:
  in-memory store, opaque cookie ID, `__Host-` + `SameSite=Strict`,
  signed double-submit CSRF via `axum_csrf`, re-login on restart
  by design. Remove the AEAD-cookie / `csrf_secret`-binding
  paragraph.
- Update `crates/backend-backendd/src/auth/tests.rs` and
  `admin_api/csrf/tests.rs`:
  - Drop tests for removed functions (encrypt/decrypt, cookie
    renderers, `SessionRevocations`).
  - Add tests against the new `AuthnBackend` impl: rate limit,
    dummy-hash timing path, success path, unknown-user path,
    revoked-password floor.
  - Keep rate-limiter and password-hash tests.
- Update the VM test (`nix/tests/test.py`) if it asserts on the
  AEAD cookie shape, session-key file path, or CSRF cookie
  internals. The endpoint surface and login UX are unchanged.

## Out of scope

- Frontend changes. The SPA continues to read `__Host-uw-csrf` and
  send `X-CSRF-Token` exactly as today. The session cookie's
  contents change from AEAD ciphertext to an opaque ID, but the
  cookie name and attributes do not, so no FE code change is
  required. Token-shape assertions in FE tests (if any) may need
  updating.
- Persistent session storage (out by epic decision).
- mTLS runtime API — no changes.
- TOTP / MFA — landing post-epic.

## Acceptance criteria

- [ ] `rg -n 'encrypt_session|decrypt_session|SessionPayload|SessionRevocations|render_session_cookie|render_csrf_cookie' crates/backend-backendd/src/`
      returns nothing.
- [ ] `rg -n 'chacha20poly1305' crates/backend-backendd/`
      returns nothing in non-`Cargo.lock` files (the dep is
      removed from `Cargo.toml`).
- [ ] `tower_sessions`, `axum_login`, and `axum_csrf` (or the
      chosen equivalent) are imported in
      `crates/backend-backendd/src/`.
- [ ] The session-key file path is not referenced anywhere in
      `crates/backend-backendd/`, `nix/modules/`, `docs/`, or
      `examples/`.
- [ ] `auth.rs` retains `verify_password`, `hash_password`,
      `DUMMY_PASSWORD_HASH`, `RateLimiter`, and the
      `LOGIN_RATE_LIMIT_*` constants.
- [ ] Admin login: with valid credentials, response sets
      `__Host-uw-session` (opaque ID) and `__Host-uw-csrf` cookies
      with `Secure; HttpOnly; SameSite=Strict; Path=/` (session)
      and `Secure; SameSite=Strict; Path=/` (csrf); status 200;
      body matches today's login response.
- [ ] Admin login: with wrong password, response status 401 and
      Argon2 verify ran (timing parity by running against
      `DUMMY_PASSWORD_HASH`) — verify via a unit test that the
      backend's `authenticate` path executes a verify on the
      unknown-user branch.
- [ ] Per-username rate limit (5 attempts / 60 s) still fires on
      repeated failures — preserved test in `auth/tests.rs`.
- [ ] Logging out a session removes it from the in-memory store;
      subsequent requests with the same cookie return 401.
- [ ] Changing user A's password or deleting user A invalidates
      all of user A's existing sessions on the next request.
- [ ] CSRF: `POST` / `PUT` / `DELETE` without `X-CSRF-Token`
      returns 403. With a header that doesn't match the cookie
      returns 403. With matching values: passes.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` and
      `cargo clippy --all-targets --workspace -- -D warnings` pass.
- [ ] `cd web && npm test` and `cd web && npm run build` pass
      without source changes in `web/`.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes (including
      after VM-test updates for the removed session-key file).
- [ ] `docs/security.md` describes the new session and CSRF model;
      no references to AEAD session cookies, `csrf_secret`
      binding, or session-key files remain in `docs/` or
      `examples/`.
