# Task: Extract `storage/documents` and `storage/secret_entries`

Parent: `700_story_backend_modularization.md`

## Scope

Pull the backend-side helpers that operate over `dyn StorageBackend`
into their own module. These are the document load/save functions and
the per-key secret-entry CRUD that today is duplicated across the
runtime and admin handlers. Doing this before splitting handlers
means each handler module can `use crate::storage::*;` instead of
reaching back into `server.rs`.

### `crates/backend-backendd/src/storage/mod.rs` (new)

Declares `documents` and `secret_entries` submodules. Re-exports the
helpers at `crate::storage::*`.

### `crates/backend-backendd/src/storage/documents.rs` (new)

Move verbatim from `server.rs`:

- `load_policy_document` (1363-1370)
- `load_resources_document` (1372-1391)
- `save_resources_document` (1393-1402)
- `load_pairs_document` (1404-1423)
- `save_pairs_document` (1425-1434)
- `load_policy_document_or_default` (4776-4781)
- `save_policy_document` (4783-4793)
- `resource_secret_dependents` (4849-4861)

All `pub(crate)`.

### `crates/backend-backendd/src/storage/secret_entries.rs` (new)

Move verbatim from `server.rs`:

- `read_resource_entry` (1086-1093)
- `write_resource_entry` (1097-1110)
- `delete_resource_entry` (1113-1125)
- `list_secret_entries` (1130-1171)
- `resource_blob_entries` (1176-1182)
- `validate_and_compose_secret_id` (1188-1219)

All `pub(crate)`.

### Call-site updates

`server.rs` now imports these from `crate::storage::*`. No semantic
changes.

## Acceptance criteria

- [ ] `storage/mod.rs`, `storage/documents.rs`,
      `storage/secret_entries.rs` exist with the listed contents.
- [ ] `server.rs` no longer defines any of the listed functions.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.

## Out of scope

- Adding new helpers that don't already exist (e.g. a
  `load_or_default` variant for documents that don't currently have
  one). Pure move only.
- Touching `shared-store`. These functions wrap the existing
  `underwrap_store::*` helpers; no signatures change in
  `underwrap_store`.
