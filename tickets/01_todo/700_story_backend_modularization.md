# Story: Split `backend-backendd/src/server.rs` into focused modules

Parent: `epic_backend_modularization.md`

## Scope

Move the 6 300 lines of production code currently in
`crates/backend-backendd/src/server.rs` into the module layout
described in the epic, in twelve mechanical PRs. The `#[cfg(test)]`
block stays in place until the final task relocates it.

Each task in this story is intended to be one PR that:

1. Creates the new module(s) and moves the listed functions / types
   verbatim.
2. Updates `lib.rs` declarations and `pub(crate) use` re-exports so
   call sites elsewhere in the crate compile.
3. Leaves `server.rs` smaller by exactly the moved content and a
   reference to the new module name.
4. Keeps `cargo test --workspace`, `cargo fmt --check`, and
   `cargo clippy --all-targets --workspace -- -D warnings` green.

The tasks are ordered so each one depends only on earlier ones:

1. `710_task_extract_config_store_registry_tls.md` — pure data + TLS.
2. `720_task_extract_http_responses_origin.md` — transport helpers.
3. `730_task_extract_runtime_state_csrf_audit.md` — cross-cutting glue.
4. `740_task_extract_storage_helpers.md` — `StorageBackend` helpers.
5. `750_task_extract_runtime_api.md` — three runtime endpoints.
6. `760_task_extract_admin_auth_bootstrap.md` — admin login surface.
7. `770_task_extract_admin_small_subdomains.md` — users, overview,
   secrets, policy, pairs, permissions.
8. `780_task_extract_admin_resources.md` — resource CRUD.
9. `790_task_extract_admin_client_instances.md` — client-instance CRUD.
10. `800_task_extract_admin_stores.md` — store CRUD (the biggest).
11. `810_task_admin_request_router_refactor.md` — flatten
    `handle_admin_request` into per-prefix sub-routers.
12. `820_task_relocate_unit_tests.md` — move each test to live next
    to the code it exercises.

## Acceptance criteria

- [ ] All twelve task tickets are done.
- [ ] `crates/backend-backendd/src/server.rs` no longer exists.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.
- [ ] The largest single file in `crates/backend-backendd/src/` is on
      the order of 1 200 lines or fewer.

## Out of scope

See the epic. In particular: no behavioural change, no public API
change, no `AdminRuntimeState` split, no rename / signature change to
moved functions.
