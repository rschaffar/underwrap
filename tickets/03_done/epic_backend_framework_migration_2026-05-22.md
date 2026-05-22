# Epic: Migrate backend HTTP layer to Axum + tower + OpenAPI (2026-05-22)

## Background

`crates/backend-backendd/` is built directly on `hyper` with a
hand-rolled accept loop and inline `if/else if path == ...`
dispatch in `admin_api/mod.rs:53–176`. `axum` is already declared
as a workspace dependency but unused. The hand-rolled surface
includes:

- **HTTP plumbing**: manual `hyper::server::conn::http1::Builder`
  TLS accept loop in `server.rs`; per-subdomain `route()` functions
  in `admin_api/{users,stores,client_instances,secrets,resources,
  pairs,policy,overview,auth}.rs` that each match `(method, path)`
  inline.
- **Middleware chain**: SPA fallback → origin check → public route
  shortcut → session auth → CSRF check → principal resolve →
  dispatch → response CSRF cookie refresh, all sequenced inline.
- **Session layer**: bespoke ChaCha20-Poly1305 AEAD-encrypted
  cookie + `SessionRevocations` in `auth.rs`. The csrf_secret is
  embedded in the session payload, giving stricter-than-typical
  double-submit CSRF (header == cookie AND header == session
  secret). Sound, but ties CSRF and session implementations
  together so they have to change together.
- **CSRF layer**: 179-line `admin_api/csrf.rs` carrying the
  three-way check.
- **SPA hosting**: 218-line hand-rolled `web.rs` with embedded +
  disk modes, MIME detection, cache-control rules, history
  fallback, path-traversal protection.
- **API contract**: ts-rs emits ~100 TypeScript type files into
  `web/src/api/types/`, but there is no OpenAPI spec. The frontend
  re-types every operation by hand in `web/src/api/client.ts`
  (~340 lines) with manual CSRF header injection.

Discussion in the planning session settled on:

1. The hand-rolled HTTP, middleware, SPA, and FE-client plumbing
   are reinventing well-established libraries. Replace.
2. The stateless AEAD-cookie session design buys nothing for a
   single-process NixOS daemon. Swap to `tower-sessions` with the
   in-memory store. Re-login after restart is acceptable.
3. The bespoke CSRF check moves to `axum_csrf`. Functionally
   equivalent at OWASP's current recommended tier.
4. Argon2id password hashing, constant-time dummy hash, the
   per-username login rate limiter, `__Host-` cookie prefix and
   `SameSite=Strict` all stay — those are either already a crate
   underneath or are small patterns worth preserving.
5. mTLS runtime API (`/v1/wrappers*`) is untouched.

## Goals

1. **Put the backend on standard chassis.** Axum Router + tower
   layers replace the hand-rolled `hyper` server and the inline
   middleware chain.
2. **Adopt standard auth wiring.** `tower-sessions` (in-memory
   store) + `axum-login` for sessions; `axum_csrf` for the CSRF
   double-submit; existing Argon2id verification and the
   per-username rate limiter stay.
3. **Replace SPA hosting** (`web.rs`) with `axum-embed` for the
   embedded build and `tower-http::services::ServeDir` for the
   dev disk path.
4. **Generate an OpenAPI spec** from the typed Axum handlers
   using `utoipa` + `utoipa-axum`. The spec becomes the single
   source of truth for the FE.
5. **Generate the FE API client** from the OpenAPI spec with
   `openapi-fetch`. Delete `web/src/api/types/` and the
   request-typing layer of `web/src/api/client.ts`.

## Stories

- `1700_story_backend_framework_migration.md` — single rollout
  story enumerating the task tickets and their landing order.

## Product decisions

- **No security model regression.** Argon2id, `__Host-` cookies,
  `SameSite=Strict`, constant-time compare on CSRF, constant-time
  dummy hash, per-username login rate limit, mTLS for runtime API
  all preserved. Three-way CSRF check is replaced by standard
  signed double-submit (`axum_csrf`); this is OWASP-current and
  cryptographically equivalent.
- **In-memory session store only.** No SQLite/disk/Redis. Operators
  log in again after a daemon restart. See
  `[[project_session_store_decision]]`.
- **Per-IP rate limiting is out of scope.** Per-username covers the
  realistic threat model for ≤5 operators; per-IP would be
  marginal. See planning discussion.
- **No external IDP, no MFA in this epic.** TOTP/MFA can land later
  as feature work on top of the new stack without architectural
  rework — see planning discussion on lock-in.
- **One PR per task.** Each task ticket leaves the tree green
  (`cargo test --workspace`, `cargo fmt -- --check`,
  `cargo clippy --all-targets --workspace -- -D warnings`,
  `cd web && npm test`, `cd web && npm run build`; the FE-client
  task additionally keeps `cd web && npm run test:e2e` green).
- **No parallel-with-legacy intermediate states.** Per
  `[[feedback_no_legacy_cutover]]`, each task is a complete
  cutover. Per `[[feedback_end_result_over_half_finished]]`, each
  task is its own session.
- **Sequencing matters.** 1710 (Axum chassis) blocks the rest.
  1740 (OpenAPI spec) blocks 1750 (FE client). 1720, 1730, 1740
  are independent of each other once 1710 lands.

## Acceptance criteria

- [ ] All 5 task tickets (1710–1750) are in `03_done/`.
- [ ] `axum` is imported in `crates/backend-backendd/src/`; the
      hand-rolled `hyper::server::conn::http1::Builder` accept
      loop is gone.
- [ ] `crates/backend-backendd/src/auth.rs` no longer contains
      `encrypt_session` / `decrypt_session` / `SessionRevocations`.
- [ ] `crates/backend-backendd/src/admin_api/csrf.rs` is gone (or
      reduced to a thin axum_csrf adapter).
- [ ] `crates/backend-backendd/src/web.rs` is ≤ ~50 lines.
- [ ] An OpenAPI spec file is emitted by the backend build (path
      decided in 1740).
- [ ] `web/src/api/types/` is gone; FE imports go through the
      generated `openapi-fetch` client.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` and
      `cargo clippy --all-targets --workspace -- -D warnings` pass.
- [ ] `cd web && npm test`, `cd web && npm run build`,
      `cd web && npm run test:e2e` all pass.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.

## Out of scope

- TOTP / MFA enrollment and verification.
- External IDP integration (Authelia, Keycloak, OIDC).
- Per-IP rate limiting.
- Persistent session storage.
- WebAuthn / passkeys.
- Password reset via email.
- mTLS runtime API (`/v1/wrappers*`) — unchanged.
- Argon2id password hashing — unchanged.
- Per-username login rate limiter — unchanged.
- Constant-time dummy hash for unknown usernames — unchanged.
- Operator role / store-access model in `operators.rs` — unchanged
  (domain modeling, independent of framework).
