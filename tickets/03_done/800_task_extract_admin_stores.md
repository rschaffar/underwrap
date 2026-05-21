# Task: Extract `admin_api/stores`

Parent: `700_story_backend_modularization.md`

## Scope

Move the store admin subdomain — the biggest remaining one in
`server.rs` at ~1 100 lines. CRUD plus the lifecycle operations
(unlock, lock, rotate-key, reencrypt). One file is the right granularity
for this iteration: the operations share private helpers
(`rotation_lock`, `resolve_store_dir`, `do_admin_store_*`,
`reencrypt_into`) and splitting them further would obscure the
relationships. Further splitting can happen as a follow-up if the
~1 100-line file proves uncomfortable in practice.

### `crates/backend-backendd/src/admin_api/stores.rs` (new)

Move verbatim from `server.rs`:

- `backend_kind` (3159-3166)
- `saturating_u32` (3168-3171)
- `admin_store_usage` (3173-3203)
- `admin_store_actions` (3205-3231)
- `admin_store_list_item` (3233-3249)
- `admin_store_detail` (3251-3281)
- `admin_store_usage_for_locked` (3283-3300)
- `handle_admin_stores_list` (3302-3329)
- `handle_admin_store_detail` (3331-3365)
- `resolve_store_dir` (3367-3388)
- `handle_admin_store_create` (3390-3405) + `do_admin_store_create`
  (3407-3557)
- `generate_master_key_bytes` (3559-3565), `master_key_hex` (3567-3570)
- `create_data_dir_secure` (both `#[cfg(unix)]` and the non-unix
  fallback, 3572-3590)
- `handle_admin_store_update` (3592-3604) + `do_admin_store_update`
  (3606-3687)
- `handle_admin_store_delete` (3689-3799)
- `rotation_lock` (3801-3814)
- `handle_admin_store_rotate_key` (3816-4046)
- `reencrypt_into` (4048-4143)
- `handle_admin_store_unlock` (4145-4161) + `do_admin_store_unlock`
  (4163-4263)
- `handle_admin_store_lock` (4265-4293)
- `write_admin_store_audit` (4295-4312)
- `admin_store` helper (2102-2115) — used by every subdomain; move
  to `crate::admin_api::stores` and re-export, or alternatively to
  `crate::runtime_state` next to `AdminRuntimeState`. Decide in the
  PR; document the choice.

### `crates/backend-backendd/src/admin_api/mod.rs`

Declare the `stores` submodule. `handle_admin_request` updates all
`/v1/admin/stores*` arms (including the path-parameterised
`/{id}/{rotate-key,unlock,lock}` triple) to call the new locations.

## Acceptance criteria

- [ ] `admin_api/stores.rs` exists with the listed contents.
- [ ] `server.rs` no longer defines any of the listed items.
- [ ] `admin_store` lives in exactly one well-named module, called
      from the same set of sites as today.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes (this is the
      first task that touches the rotation / unlock / lock paths the
      VM test exercises end-to-end).

## Out of scope

- Splitting `stores.rs` into `stores/{crud,lifecycle,…}` submodules.
  Defer to a follow-up if needed after the file lives on its own.
- Changing the unlock / rotate-key / lock semantics.
