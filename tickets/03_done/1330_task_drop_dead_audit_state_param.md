# Task: Drop unused `_state` parameter from four audit helpers

Parent: `1300_story_backend_backendd_refinement.md`
Resolves: F-002 (review report `backend_backendd_2026-05-22.md`)

## Background

Four audit helpers take `&AdminRuntimeState` as their first argument
and rename it to `_state` because they never consult it:

- `crates/backend-backendd/src/admin_api/policy.rs:53-54` —
  `write_admin_permission_audit`.
- `crates/backend-backendd/src/admin_api/users.rs:43-44` —
  `write_admin_user_audit`.
- `crates/backend-backendd/src/admin_api/bootstrap.rs:229-234` —
  `audit_bootstrap_attempt`.
- `crates/backend-backendd/src/admin_api/stores/mod.rs:264-272` —
  `write_admin_store_audit`.

All four just hand operator / resource strings to `log_admin_event`.
Compare with `client_instances.rs:27-36` and `resources.rs:29-38`,
which read identically and *don't* take state. Ticket 1230's audit
unification collapsed the per-subdomain wrappers into a consistent
signature; this is the trailing dead parameter.

## Scope

- Drop `_state: &AdminRuntimeState` from the four helper signatures.
- Update every call site (search `crates/backend-backendd/src/` for
  `write_admin_permission_audit(`, `write_admin_user_audit(`,
  `audit_bootstrap_attempt(`, `write_admin_store_audit(`).
- Update `test_support::run_bootstrap` if it still calls
  `audit_bootstrap_attempt(state, ...)`.

## Out of scope

- Other changes to the audit-event shape (ticket 1230 already
  landed those).
- Adding new audit fields.

## Acceptance criteria

- [ ] `rg -n '_state: &AdminRuntimeState' crates/backend-backendd/src/`
      returns nothing.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
