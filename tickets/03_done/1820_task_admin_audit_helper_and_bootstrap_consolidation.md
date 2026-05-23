# Task: Collapse per-subdomain audit helpers and fold `bootstrap` into `auth`

## Background

Two small structural redundancies survive in the admin tree after
the 1700-series migration and the 1800 error unification.

### Per-subdomain audit wrappers

`admin_api/audit.rs` exposes one helper, `log_admin_event(action,
operator, wrapper, resource, secret_id, operation, result)`. Every
mutation calls it. But each subdomain wraps that call in its own
near-identical "format the entity string then forward most args as
`None`" function:

- `admin_api/users.rs::write_admin_user_audit`
- `admin_api/client_instances.rs::write_admin_client_instance_audit`
- `admin_api/resources.rs::write_admin_resource_audit`
- `admin_api/pairs.rs::write_admin_pair_audit` (approx; see file)
- `admin_api/secrets.rs::write_admin_secret_audit` (two arities)
- `admin_api/policy.rs::write_admin_permission_audit`
- `admin_api/stores/mod.rs::write_admin_store_audit`

They differ only in which slot of `log_admin_event` they fill and
how the entity id is prefixed (`user:`, `client_instance:`,
`resource:`, `wrapper:`, `secret:`, `permission:…`, `store:`). The
shape is mechanical — seven copies of essentially the same five
lines.

`admin_api/bootstrap.rs::audit_bootstrap_attempt` is genuinely
different (it carries a `username` argument with no entity record
yet), so it stays.

### `bootstrap.rs` as a top-level subdomain

`auth.rs` (297 lines, login / whoami / logout / password change) and
`bootstrap.rs` (223 lines, bootstrap status + first-operator create)
share the same path prefix (`/v1/admin/auth/*` and `/v1/admin/auth/
bootstrap/*`), the same login-cookie shape, and the same public-
router membership. The only thing setting bootstrap apart is the
"once until any operator exists" guard. Splitting it into its own
top-level module pulls them apart at the file system but not at the
URL space.

## Scope

One PR with two independent refactors. Either could be a standalone
PR if reviewers prefer; bundling makes sense because both touch
`admin_api/mod.rs`'s subdomain list and the `build_*_router`
helpers, and both are pure refactors of the same surface.

### Part A — single audit helper

1. Replace the seven `write_admin_*_audit` wrappers with one helper
   on `admin_api/audit.rs`. Sketch:

   ```rust
   pub(crate) enum AuditEntity<'a> {
       User(&'a str),
       Store(&'a str),
       ClientInstance(&'a str),
       Resource(&'a str),
       Wrapper(&'a str),
       Secret(&'a str),
       Permission { store: &'a str, perm: &'a WrapperPermission },
   }

   pub(crate) fn log_admin_event(
       action: &str,
       operator: &Principal,
       entity: AuditEntity<'_>,
       operation: Option<&str>,
       result: &str,
   ) { ... }
   ```

   Exact shape is the implementer's call — what matters is that the
   `entity:{id}` formatting and the `Some(_)`/`None` slot mapping
   live in one place. Keep `operator: &Principal` so call sites stop
   spelling out `principal.username.as_str()`.

2. Update every handler call site to use the unified helper. There
   are roughly 30 of them; most are mechanical.

3. Delete the seven `write_admin_*_audit` functions.

4. The wire shape of the `tracing::info!` line (field names + their
   `Some`/`None`-ness) must be preserved exactly — operators grep
   for these. Add a snapshot or assertion in `audit/tests.rs`
   (create the sibling file if needed) that exercises each
   `AuditEntity` variant and asserts the emitted fields.

### Part B — fold `bootstrap` into `auth`

1. Move the contents of `admin_api/bootstrap.rs` into
   `admin_api/auth/bootstrap.rs`. Convert `auth.rs` into
   `auth/mod.rs` (with the existing private items kept in
   `auth/mod.rs` or split further only if it helps readability —
   don't over-engineer).

2. Merge the two `public_router()` definitions into one
   `admin_api::auth::public_router()` that registers both the
   login/whoami routes and the bootstrap status/create routes.

3. Update `admin_api/mod.rs::build_public_router` to call the
   single merged `auth::public_router()`. Drop the `bootstrap`
   line from `build_public_router` and the `pub mod bootstrap;`
   declaration.

4. `audit_bootstrap_attempt` moves with its file and stays
   bootstrap-specific.

5. URL paths and OpenAPI tags do not change. The OpenAPI drift
   snapshot must remain byte-identical (`openapi_export.rs`).

## Out of scope

- Any wire-shape change: status codes, request/response bodies,
  cookie behavior, audit log field names.
- Any change to the still-public top-level subdomains
  (`users`, `stores`, `client_instances`, `resources`, `secrets`,
  `pairs`, `policy`, `csrf`).
- Touching the runtime API.
- Restructuring `admin_api/csrf.rs` or the layer composition in
  `mod.rs`.
- Adding new audit fields. If a slot is unused today (`wrapper` on
  a user mutation, etc.) it stays unused.

## Acceptance criteria

- [ ] `grep -rn "write_admin_.*_audit" crates/backend-backendd/src/`
      returns zero hits.
- [ ] `admin_api/bootstrap.rs` no longer exists as a top-level
      sibling of `auth.rs`; bootstrap code lives under
      `admin_api/auth/bootstrap.rs`.
- [ ] `admin_api/mod.rs` no longer declares a top-level
      `bootstrap` module.
- [ ] OpenAPI drift snapshot is byte-identical (no regeneration
      needed).
- [ ] The `tracing::info!("admin event", ...)` field layout is
      covered by a test in `admin_api/audit/tests.rs` (or
      equivalent) for at least one variant per `AuditEntity` arm.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` and
      `cargo clippy --all-targets --workspace -- -D warnings` pass.
- [ ] `cd web && npm test` passes (no FE changes expected; this
      catches accidental wire drift).
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.

## Notes for the implementer

- This depends on ticket 1810 being merged first (or developed on
  top of it) — 1810 deletes the `overview.rs` module entirely, and
  there is no point migrating its audit helper if it is about to
  disappear. Rebase if 1810 lands first; otherwise mention the
  ordering in the PR description.
- Suggested commit sequence inside the PR:
  1. Introduce the unified `log_admin_event(..., AuditEntity, ...)`
     signature; keep the old `write_admin_*_audit` wrappers as
     one-line forwarders so the migration compiles incrementally.
  2. Migrate call sites subdomain by subdomain.
  3. Delete the wrappers.
  4. Move `bootstrap.rs` under `auth/`.
- Keep `audit_bootstrap_attempt` as a free function in the new
  `auth/bootstrap.rs`. It has no entity record at call time and
  doesn't fit the `AuditEntity` enum cleanly.
- Do not "improve" audit-field semantics in the same PR. If we
  decide later that, say, secret mutations should also carry the
  parent resource id, that is a separate ticket with intentional
  log-grep impact.
