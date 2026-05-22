# Task: Split `stores.rs` into `stores/{mod,crud,lifecycle}.rs`

Parent: `1130_story_admin_api_refinement.md`
Resolves: F-001, F-002 (review report `admin_api_2026-05-22.md`)

## Background

`crates/backend-backendd/src/admin_api/stores.rs` is 1 289 lines and
mixes two concerns:

- **CRUD**: list, detail, create, update, delete, plus the
  `admin_store_*` projector helpers and the `admin_store` /
  `resolve_store_dir` resolvers (the first ~710 lines).
- **Lifecycle**: rotate-key, unlock, lock, the `reencrypt_into`
  helper, and the rotation `Mutex` (the last ~480 lines).

The `CLAUDE.md` ceiling is ~600 lines. The matching `stores/tests.rs`
is 747 lines and mixes CRUD-shaped tests with rotate-key / unlock /
lock tests in the same way.

User decision 2026-05-22: split three ways
(`mod.rs` / `crud.rs` / `lifecycle.rs`).

## Scope

Production split. Move into
`crates/backend-backendd/src/admin_api/stores/`:

- `stores/mod.rs`:
  - `route()` dispatcher (it carries the path-strip-suffix matchers
    for `/rotate-key`, `/unlock`, `/lock` and stays the public surface).
  - `admin_store()` (the shared store-handle resolver).
  - `admin_store_actions`, `admin_store_usage`,
    `admin_store_usage_for_locked`, `admin_store_list_item`,
    `admin_store_detail`, `backend_kind`, `saturating_u32`,
    `resolve_store_dir` — used by both halves for response building.
  - `write_admin_store_audit`, `create_data_dir_secure` — shared.
  - `pub(super) mod crud; pub(super) mod lifecycle;` declarations.
- `stores/crud.rs`:
  - `handle_admin_stores_list`, `handle_admin_store_detail`,
    `handle_admin_store_create` (+ its `_endpoint` sibling post-1140),
    `handle_admin_store_update`, `handle_admin_store_delete`, plus
    any helpers private to CRUD.
- `stores/lifecycle.rs`:
  - `handle_admin_store_rotate_key`, `reencrypt_into`,
    `rotation_lock`, `handle_admin_store_unlock`,
    `handle_admin_store_lock`, plus `generate_master_key_bytes` and
    `master_key_hex` (their only use was during encrypted-store
    create + rotate-key; move them here and have `crud.rs` import
    via `pub(super) use` if it still needs `master_key_hex` for
    create, or inline the six-line helper there — judgement call).

Tests split. Move into
`crates/backend-backendd/src/admin_api/stores/crud/tests.rs` and
`crates/backend-backendd/src/admin_api/stores/lifecycle/tests.rs`:

- Re-declare from `crud.rs` and `lifecycle.rs` with
  `#[cfg(test)] mod tests;`.
- Split the existing 747-line `stores/tests.rs` so each new file
  lands under ~400 lines. CRUD-shaped tests go to `crud/tests.rs`;
  rotate-key/unlock/lock tests go to `lifecycle/tests.rs`. Any
  fixture or helper used by both halves moves to a shared spot —
  either `stores/tests_support.rs` (declared once from `stores/mod.rs`
  with `#[cfg(test)] mod tests_support;`) or `crate::test_support`
  if it generalises that far.
- If there's an existing top-of-file `mod tests {}` block elsewhere
  in `stores.rs` (vs. the sibling `stores/tests.rs`), this ticket
  collapses to "split the sibling". Verify before starting.

Update every `use crate::admin_api::stores::...` import elsewhere in
the crate to point at the new path. Most callers only reach for
`admin_store`, which stays in `stores/mod.rs` and re-exports
identically — those should need no change.

## Out of scope

- Any logic change inside the moved code (this is the biggest move
  in the epic; keep it mechanical).
- Renaming `do_admin_*` (1140 already handled that).
- Touching `stores.rs`-adjacent modules like `client_instances.rs`,
  even where they share helpers — those stay where they are.
- Re-shaping `route()` or its dispatch shape.

## Acceptance criteria

- [ ] `crates/backend-backendd/src/admin_api/stores.rs` no longer
      exists as a flat file; the `stores/` directory has `mod.rs`,
      `crud.rs`, `lifecycle.rs`.
- [ ] Each of `crud.rs`, `lifecycle.rs` and the test siblings is
      under 600 lines.
- [ ] `mod.rs` is under 400 lines.
- [ ] No production code is duplicated between `crud.rs` and
      `lifecycle.rs`; shared helpers live in `mod.rs` (or are
      `pub(super)` imports across the two).
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
