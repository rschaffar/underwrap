# Story: Roll in-memory master keys across protocol, backend, SPA, tests, docs

Parent: `epic_in_memory_master_keys.md`

## Scope

End-to-end change to keep encrypted-store master keys in memory only.
Each task ticket below carves out one self-contained piece. They are
ordered so each ticket builds on the previous and leaves the tree in a
buildable state.

## Task tickets

- `510_task_drop_key_file_from_storage_config.md` — protocol/schema
  change. `StorageConfig::Encrypted` loses `key_file`;
  `EncryptedBackend::open_with_key` replaces `open`; `shared-store`
  tests migrate.
- `520_task_store_registry_lock_state.md` — `StoreRegistry` carries a
  `StoreState::{Locked, Unlocked}` enum; every backend call site uses
  a helper that returns `StoreLocked` instead of pattern matching.
- `530_task_store_locked_error_code.md` — `ErrorCode::StoreLocked`
  wire encoding + service HTTP mapping (503 + `{"code":"store_locked"}`).
- `540_task_admin_unlock_lock_endpoints.md` — `POST .../unlock` (hex
  body) and `POST .../lock`. Canary-based key validation. Audit-logged.
- `550_task_create_store_no_disk_key.md` — `handle_admin_store_create`
  generates the key in memory only, writes the canary, returns hex
  once, deletes the `keys_dir` plumbing.
- `560_task_rotate_key_reencrypts.md` — rotation re-encrypts every
  blob under the new key, holds a per-store write lock, rolls back on
  failure. Refuses on locked stores.
- `570_task_spa_locked_state.md` — admin SPA: locked badge, Unlock /
  Lock dialogs, rotate dialog warning, Vitest coverage.
- `580_task_vm_test_locked_store.md` — NixOS VM test: create →
  restart → wrapper sees `StoreLocked` → unlock → wrapper reads →
  rotate → wrapper still reads same plaintext.
- `590_task_docs.md` — `docs/backend.md`, `docs/security.md`,
  examples, CLAUDE.md updates.

## Acceptance

- All task tickets closed.
- `cargo fmt -- --check`, `cargo clippy --all-targets --workspace -- -D
  warnings`, and `cargo test --workspace` green.
- `cd web && npm test` green; Playwright e2e green inside `nix
  develop`.
- `nix build .#checks.x86_64-linux.vm-test` green, including the new
  locked-store case.

## Out of scope

Same as the epic: no passphrase / KDF, no TPM, no quorum, no
auto-relock.
