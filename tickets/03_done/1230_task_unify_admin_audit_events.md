# Task: Unify admin audit-event shape across admin_api

Resolves: F-009 (review report `admin_api_2026-05-22.md`)

## Background

`admin_api/` currently emits audit events three different ways:

1. **Per-subdomain `write_admin_*_audit` helper** that wraps
   `log_admin_event` — `users.rs`, `stores/mod.rs`, `policy.rs`,
   `bootstrap.rs`.
2. **Inline `log_admin_event` calls** — `pairs.rs` (3 sites),
   `resources.rs` (5 sites), `client_instances.rs` (3 sites).
3. **Raw `tracing::info!` with a different field schema** —
   `secrets.rs::handle_admin_secret_write` and
   `handle_admin_secret_rename` emit `audit = "secret_stored"` /
   `"secret_renamed"` with `operator`, `store`, `secret` /
   `from` / `to` fields. None of these go through
   `log_admin_event`.

The `resource` field's content is also inconsistent: some sites
format it as `<type>:<key>` (`user:bob`, `store:work`,
`wrapper_permission:...`, `client-instance:...`), some pass a bare
id (`pairs.rs` puts the resource id in the `resource` field with no
prefix; `resources.rs` passes the bare resource id or name).

Bootstrap's action string is also out of pattern: `bootstrap_attempt`
(snake_case) vs `user.create` / `store.update` / `pair.delete` etc.
(dotted lowercase).

## Scope

**Canonical shape** (after this ticket):

- Every audit event goes through `crate::admin_api::audit::log_admin_event`.
- `action` is dotted lowercase: `<subject>.<verb>`. Examples:
  `user.create`, `store.unlock`, `client-instance.update`,
  `pair.create`, `resource.delete`, `secret.write`,
  `bootstrap.attempt`.
- `resource` is always `<type>:<key>` and is the canonical handle
  for the entity. For composite-keyed entities, slashes separate
  the components. Examples:
  - `user:<username>`
  - `store:<name>`
  - `client-instance:<id>`
  - `wrapper_permission:<store>/<inst>/<wrapper>/<resource_id>`
  - `pair:<wrapper>/<resource_id>`
  - `resource:<id>`
  - `secret:<store>/<resource_id>/<slot>/<key>`

**Per-subdomain helpers.** Add `write_admin_pair_audit`,
`write_admin_resource_audit`, `write_admin_client_instance_audit`,
and `write_admin_secret_audit` alongside the existing
`write_admin_*_audit` helpers. Each formats the resource string
according to the convention above and calls `log_admin_event`.
Convert every inline `log_admin_event` call inside `admin_api/` to
go through a per-subdomain helper. Bootstrap's helper stays;
rename its action to `bootstrap.attempt`.

**Secrets.** Replace both raw `info!(audit = ...)` calls in
`secrets.rs` with `write_admin_secret_audit` invocations. Also emit
audit events on the failure paths of write / rename (currently
silent on failure) so secret operations match the success+failure
posture other subdomains already follow. Read / list stay silent
(other subdomains' read-only paths emit no audit either).

## Out of scope

- Backfilling missing audit emissions in non-secret subdomains
  beyond what the failure-path consistency requires (no audit
  changes on read-only endpoints; no audit on `secret.read`,
  `secret.list`).
- Changing `log_admin_event`'s signature.
- Daemon-side audit events (`frontend-underwrapd/`).
- Adding a new persisted audit log; this is purely the
  `tracing::info!(target="admin event")` event shape.

## Acceptance criteria

- [ ] No `tracing::info!(audit = ...)` call remains in `admin_api/`.
- [ ] No bare `log_admin_event(...)` call site inside `admin_api/`
      outside of `audit.rs` itself and the per-subdomain helpers.
- [ ] Every audit `resource` field, when populated, matches the
      `<type>:<key>` convention above.
- [ ] Bootstrap action is `bootstrap.attempt`.
- [ ] Secret write and rename emit audit events on both success
      and failure; secret delete emits on success and failure too.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
