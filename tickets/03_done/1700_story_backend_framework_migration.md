# Story: Roll out the backend framework migration

Parent: `epic_backend_framework_migration_2026-05-22.md`

## Scope

Land the 5 task tickets below, in the order shown. Each task is
one PR that:

1. Resolves the scope described in its own ticket.
2. Keeps `cargo test --workspace` green.
3. Keeps `cargo fmt -- --check` and
   `cargo clippy --all-targets --workspace -- -D warnings` green.
4. Keeps `cd web && npm test` and `cd web && npm run build` green.
5. For 1750 only: also keeps `cd web && npm run test:e2e` green.
6. For 1710 and 1720: also keeps
   `nix build .#checks.x86_64-linux.vm-test` green.
7. Does not regress the security model
   (see epic "Product decisions").

Tasks are ordered so each one depends only on earlier ones:

1. `1710_task_port_admin_api_to_axum_router.md` — Replace the
   hand-rolled hyper server + manual path dispatch with an
   `axum::Router`. Wrap existing auth / CSRF / principal-resolve
   helpers as `tower::Layer`s. Pure structural refactor: same
   endpoints, same security checks, same wire formats. The session
   AEAD code, custom CSRF module, SPA hosting and ts-rs FE client
   all stay in place during this step.
2. `1720_task_migrate_sessions_and_csrf.md` — Swap the bespoke
   AEAD session cookie + `SessionRevocations` for `tower-sessions`
   (in-memory store) + `axum-login`. Replace the custom CSRF
   module with `axum_csrf`. Removes `auth.rs`'s session
   encrypt/decrypt code and `admin_api/csrf.rs` entirely (or
   reduces csrf.rs to a thin adapter). Argon2id verify,
   per-username rate limiter and constant-time dummy hash stay.
3. `1730_task_replace_spa_hosting_with_axum_embed.md` —
   Replace `web.rs` (~218 lines) with `axum-embed` for the
   embedded `rust-embed` source and `tower-http::services::ServeDir`
   (+ `ServeFile` fallback) for the dev disk source. SPA history
   fallback, ETag/304, cache-control headers all come from the
   library layer. Independent of 1720.
4. `1740_task_add_utoipa_openapi_spec.md` — Add `utoipa-axum`
   annotations to every admin handler. Emit an OpenAPI JSON spec
   at build time. Backend only; FE still uses ts-rs in this step.
   Independent of 1720 and 1730 — depends only on 1710.
5. `1750_task_generate_fe_client_from_openapi.md` — Replace
   `web/src/api/types/` (ts-rs output) and the hand-rolled
   `web/src/api/client.ts` request-typing layer with an
   `openapi-fetch` client generated from the spec produced in
   1740. CSRF header injection becomes a single middleware.
   Update all FE call sites.

1720, 1730 and 1740 are independent of each other once 1710 has
landed. Suggested order is the one above (sessions/CSRF first
because it touches the most behavior; SPA hosting second because
it is small and self-contained; OpenAPI annotations last because
they want a stable handler surface).

## Acceptance criteria

- [ ] All 5 task tickets (1710–1750) are in `03_done/`.
- [ ] `axum` and `tower` are imported in
      `crates/backend-backendd/src/`; no manual
      `hyper::server::conn::http1::Builder` accept loop remains.
- [ ] `auth.rs` no longer carries `encrypt_session`,
      `decrypt_session`, `SessionRevocations`.
- [ ] `admin_api/csrf.rs` is gone or is a thin axum_csrf adapter.
- [ ] `web.rs` is ≤ ~50 lines.
- [ ] An OpenAPI spec is emitted at build time (path defined in
      1740).
- [ ] `web/src/api/types/` does not exist.
- [ ] `cargo test --workspace`, `cargo fmt -- --check`,
      `cargo clippy --all-targets --workspace -- -D warnings` pass.
- [ ] `cd web && npm test`, `cd web && npm run build`,
      `cd web && npm run test:e2e` pass.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.

## Out of scope

See the epic.
