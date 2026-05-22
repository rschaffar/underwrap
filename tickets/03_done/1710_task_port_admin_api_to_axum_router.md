# Task: Port the admin API to `axum::Router` + tower layers

Parent: `1700_story_backend_framework_migration.md`

## Background

`crates/backend-backendd/Cargo.toml` already declares
`axum = { workspace = true }`, but `axum` is not imported anywhere
in `crates/backend-backendd/src/`. The HTTP layer is built directly
on `hyper`:

- `crates/backend-backendd/src/server.rs` runs the accept loop on
  `hyper::server::conn::http1::Builder` directly, splitting admin
  and runtime listeners.
- `crates/backend-backendd/src/admin_api/mod.rs:53–176` is a
  single `handle_admin_request` function whose body is the entire
  middleware chain expressed as an `if let Err ... return` river:
  SPA fallback → origin check → public auth/bootstrap routes →
  session-cookie auth → CSRF check → principal resolve → logout
  shortcut → URL-prefix dispatch → CSRF cookie refresh.
- Each subdomain under `crates/backend-backendd/src/admin_api/`
  exposes a `route(method, path, state, principal, request)`
  function with its own inline `match (method, path)` block:
  `users.rs`, `stores.rs`, `client_instances.rs`, `secrets.rs`,
  `resources.rs`, `pairs.rs`, `policy.rs`, `overview.rs`,
  `auth.rs` (the password-change branch), plus the bootstrap and
  whoami handlers in `admin_api/auth.rs` and
  `admin_api/bootstrap.rs`.
- The runtime API in `crates/backend-backendd/src/runtime_api/`
  follows the same pattern with its own `handle_service_request`.

This task moves all of this onto the standard Axum + tower stack
without changing security behavior, wire formats, or endpoints.

## Scope

- Replace the hand-rolled accept loop in `server.rs` with
  `axum::serve(...)` (or equivalent) running on the configured TLS
  listeners. The admin listener and the mTLS runtime listener
  remain separate; only the chassis changes.
- Build the admin surface as an `axum::Router`:
  - A `public` sub-router carrying `POST /v1/admin/auth/login`,
    `GET /v1/admin/auth/whoami`,
    `GET /v1/admin/auth/bootstrap/status`,
    `POST /v1/admin/auth/bootstrap`.
  - A `protected` sub-router carrying every other `/v1/admin/*`
    endpoint, with handler signatures using Axum extractors
    (`State`, `Json`, typed path params).
  - Top-level layers (apply to everything): the origin check
    currently in `http/origin.rs::check_admin_origin` becomes a
    `tower::Layer` (e.g., `from_fn(origin_check_layer)`); the
    SPA fallback is wired via `Router::fallback_service(...)`.
  - Protected-only layers: the existing
    `admin_api/csrf.rs::authenticate_admin` becomes an
    `authenticate_layer` that decrypts the session cookie and
    populates an `AdminAuth` request extension;
    `check_csrf` becomes a `check_csrf_layer`;
    `resolve_principal` becomes a `resolve_principal_layer` that
    populates `Principal` as a request extension;
    `attach_csrf_cookie` becomes a `map_response` middleware.
  - The logout endpoint (`POST /v1/admin/auth/logout`) is
    registered as a protected route — it must run *after* auth +
    CSRF have passed (preserves the current `admin_api/mod.rs:138`
    ordering).
- Per-subdomain `route()` functions are replaced by Axum handlers
  registered on the protected sub-router:
  - `Router::new().route("/v1/admin/users", get(list).post(create))`
    style, one handler per `(method, path)` pair.
  - Handler bodies move the existing JSON parse +
    business-logic + response-build into Axum-shaped handlers
    that take `State<...>`, `Extension<Principal>`,
    `Json<...>`, and return `impl IntoResponse`.
  - Path parameters that were extracted by hand from
    `path.strip_prefix(...)` become typed `Path<...>` extractors.
- The runtime listener (mTLS, under `/v1/wrappers*` and
  `/v1/wrapper-invocations*`) gets the same Axum chassis but
  keeps its own router, principal model, and behavior. Do not
  merge admin and runtime routers; they have different trust
  boundaries.
- Existing session AEAD code (`auth.rs::encrypt_session`,
  `decrypt_session`, `SessionRevocations`) stays in this task. The
  authentication layer calls into it exactly as today. Migration
  away from AEAD cookies is 1720, not this ticket.
- Existing custom CSRF module (`admin_api/csrf.rs`) stays in this
  task. The check_csrf layer wraps it. Migration to `axum_csrf`
  is 1720, not this ticket.
- Existing `web.rs` SPA hosting stays in this task. Wire it as a
  `Router::fallback_service` taking a service that calls
  `serve_web_request`. Replacement with `axum-embed` is 1730, not
  this ticket.
- Existing error response shape from `http/responses.rs::error_response`
  is preserved. Either keep the helper and call it from handlers,
  or implement `IntoResponse` for a `ServiceErrorResponse` type
  that produces the same `{code, message}` JSON.
- Logging: preserve the per-request `info!(user, method, path,
  "admin request")` line that today fires from `admin_api/mod.rs`.
  Implement as a `tower-http::trace::TraceLayer` configured to
  emit the same fields, or as a small `from_fn` layer that runs
  after principal resolution.
- Origin check, CSRF check and authentication layers MUST short-
  circuit with the same status codes and error codes as today
  (`bad_origin` → 403, `unauthorized` → 401, `session_revoked` →
  401, `csrf_required` / `csrf_mismatch` → 403).
- Update `crates/backend-backendd/src/server.rs` and `lib.rs`
  exports as needed. Remove any code that becomes unreachable
  after the migration; do not leave parallel-with-legacy paths.

## Out of scope

- Session storage change (kept in 1720).
- CSRF library change (kept in 1720).
- SPA hosting library change (kept in 1730).
- OpenAPI annotations (added in 1740).
- Frontend changes — the FE still talks to the same endpoints
  with the same JSON shapes and cookie behavior. No `web/` change
  is required.
- mTLS runtime API behavior — chassis only.

## Acceptance criteria

- [ ] `rg -n 'hyper::server::conn::http1::Builder' crates/backend-backendd/src/`
      returns nothing.
- [ ] `rg -n 'use axum' crates/backend-backendd/src/` returns
      multiple hits.
- [ ] `rg -n 'fn handle_admin_request' crates/backend-backendd/src/`
      returns nothing.
- [ ] Per-subdomain `pub(crate) async fn route` signatures in
      `admin_api/{users,stores,client_instances,secrets,resources,
      pairs,policy,overview,auth}.rs` are gone, replaced by Axum
      handler functions registered on a `Router`.
- [ ] Origin, authenticate, CSRF, principal-resolve and
      response-cookie-refresh exist as tower layers (or
      `from_fn`/`map_response` middlewares) and are applied to the
      protected sub-router.
- [ ] Public routes (login, whoami, bootstrap status, bootstrap)
      bypass the authenticate + CSRF layers; logout requires both.
- [ ] No new dependencies introduced beyond `axum`, `tower`,
      `tower-http`, `axum-extra` (cookie jar) as needed. None of
      `tower-sessions`, `axum-login`, `axum_csrf`, `axum-embed`,
      `utoipa*` land in this ticket.
- [ ] Existing security tests in
      `crates/backend-backendd/src/auth/tests.rs` and
      `admin_api/csrf/tests.rs` still pass without modification of
      the asserted behavior. Test wiring (how a request is built)
      may change.
- [ ] Wire formats unchanged: every request/response JSON shape
      and every `Set-Cookie` header value matches pre-migration
      output (verify by spot-check of an integration test or a
      curl-and-diff against the pre-migration build).
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` and
      `cargo clippy --all-targets --workspace -- -D warnings` pass.
- [ ] `cd web && npm test` and `cd web && npm run build` pass
      without source changes in `web/`.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.
