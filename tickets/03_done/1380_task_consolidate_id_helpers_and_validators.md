# Task: Consolidate id validators, hex encoders, and `now_epoch_secs` callers

Parent: `1300_story_backend_backendd_refinement.md`
Resolves: F-009, F-010, F-011 (review report `backend_backendd_2026-05-22.md`)

## Background

Three independent duplications:

### F-010 — duplicate id validators

`is_valid_store_id` (`crates/backend-backendd/src/store_registry.rs:260-272`)
and `is_valid_client_instance_id`
(`crates/backend-backendd/src/runtime_clients.rs:236-248`) implement
the same rule byte-for-byte: lowercase letters / digits / `-` / `_`,
starting with an alphanumeric.

### F-009 — hand-rolled hex encoding

Four sites roll their own `format!("{b:02x}")` loop even though
`hex` is already a dependency
(`crates/backend-backendd/Cargo.toml:38`):
- `crates/backend-backendd/src/runtime_clients.rs:71-74`
  (`cert_fingerprint_from_der`).
- `crates/backend-backendd/src/ids.rs:14-22` (`random_id`).
- `crates/backend-backendd/src/ids.rs:39-48` (`random_hex_8`).
- `crates/backend-backendd/src/admin_api/stores/mod.rs:240-242`
  (`master_key_hex`).

`crates/backend-backendd/src/users.rs:227-231` (`new_user_id`)
already uses `hex::encode` — the others should match.

### F-011 — inlined `now_epoch_secs` pattern

Six sites inline `SystemTime::now().duration_since(UNIX_EPOCH).map(|d|
d.as_secs()).unwrap_or(0)` even though
`crate::users::now_epoch_secs()` is the canonical helper used in 11
other places:
- `crates/backend-backendd/src/admin_api/auth.rs:110-113, 167-170,
  207-210` (login, logout, whoami).
- `crates/backend-backendd/src/admin_api/csrf.rs:58-61`
  (`authenticate_admin`).
- `crates/backend-backendd/src/admin_api/overview.rs:65-68` (likely
  deleted by ticket 1340 — verify before touching).
- `crates/backend-backendd/src/test_support.rs:261-264` (session
  builder).

## Scope

### F-010

- Move the shared id-validation rule into `crate::ids` as
  `is_valid_simple_id(name: &str) -> bool`.
- Replace `is_valid_store_id` and `is_valid_client_instance_id`
  bodies with one-line calls to `is_valid_simple_id`. Verify with
  `cargo build -p integration-tests` and the `ts_export` binding test
  that the public re-exports (`lib.rs:28-31` re-exports
  `is_valid_store_id`) still link cleanly — keep the wrapper or
  delete it depending on whether anything outside the crate calls it.

### F-009

- Replace each of the four hand-rolled hex loops with
  `hex::encode(&bytes)`. Same lowercase no-prefix output.

### F-011

- Replace each of the inlined `SystemTime::now()` sequences with
  `crate::users::now_epoch_secs()`. Drop any leftover
  `std::time::{SystemTime, UNIX_EPOCH}` imports that become unused.

## Acceptance criteria

- [ ] `rg -n 'is_valid_(store_id|client_instance_id)' crates/backend-backendd/src/`
      shows each function as a one-liner that delegates to
      `is_valid_simple_id`, or is gone entirely.
- [ ] `rg -n 'format!\("\{b:02x\}"' crates/backend-backendd/src/`
      returns nothing.
- [ ] `rg -n 'SystemTime::now' crates/backend-backendd/src/admin_api/ crates/backend-backendd/src/test_support.rs`
      returns nothing.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
