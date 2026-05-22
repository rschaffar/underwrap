# Task: Drop pre-first-release schema-version stamps from every persisted document

Parent: `1300_story_backend_backendd_refinement.md`
Resolves: F-001 (review report `backend_backendd_2026-05-22.md`)

## Background

Every persisted document this crate owns carries a `version: u32`
discriminator, an `ensure!` that the field equals a hard-coded
constant, and at least one `serde(default)` field with a comment
about tolerating older fixtures. No load path branches on the
version — they all just early-fail when the value doesn't match the
current constant. There is no first release yet; the older shapes
were never deployed.

Per `CLAUDE.md` "Release posture" and the
`underwrap-component-refine` skill's posture: schema-version fields,
`serde(default)` fields that exist only to tolerate older blobs, and
"older test fixtures" tolerations are all dead weight that has to
come out before the first release. Real versioning starts *after*
the first release, when there are actual deployed clients to
migrate.

## Scope

### Backend crate

- `crates/backend-backendd/src/users.rs`:
  - Delete `COLLECTION_VERSION`, `STATE_FILE_VERSION`,
    `default_state_file_version`.
  - Delete the `version: u32` field from `UserStateDocument` and the
    `version: u32` field from `UserCollection`.
  - Delete the matching `ensure!` blocks in
    `load_state_document` / `save_state_document` / `save` /
    `UserCollection::default`'s version stamp.
- `crates/backend-backendd/src/runtime_clients.rs`:
  - Delete the `version: u32` field from `ClientInstancesDocument`
    and `default_client_instances_version`.
  - Delete the matching `ensure!` in `load_client_instances`.
  - Delete the `serde(default)` on
    `CurrentCredentialRecord.issued_at` and on
    `ClientInstanceRecord.auto_create_wrapper_assets`. Update test
    fixtures and admin-create paths to pass both fields explicitly
    (they already do for the production paths; only test JSON
    fixtures may need a touch).
- `crates/backend-backendd/src/store_registry.rs`:
  - Delete the `version: u32` field from `StoreRegistryDocument` and
    the `ensure! doc.version == 1` in `load_store_records`.

### Protocol crate

- `crates/shared-protocol/src/entities.rs`:
  - Delete `ENTITY_SCHEMA_VERSION`.
  - Delete the `version: u32` field on `ResourcesDocument` and
    `PairsDocument`.
  - Delete the matching default helpers and the `ensure!` blocks
    referenced from
    `crates/backend-backendd/src/storage/documents.rs:45-50,77-82`.

### Tests

- Delete the unit tests that exercise the version-mismatch paths
  (`load_rejects_unknown_state_file_version` in
  `crates/backend-backendd/src/users/tests.rs:82`, the analogous
  tests in `runtime_clients/tests.rs`, `store_registry/tests.rs`,
  and the storage/documents tests at
  `crates/backend-backendd/src/storage/documents/tests.rs:52, 86`).
- Update any JSON fixtures inside `tests.rs` files that hard-code a
  `"version": 1` or `"version": 2` field. The simplest path is to
  delete the key — the structs will no longer accept it.
- Update integration-tests / VM-test fixtures that hand-write
  JSON into these files (search
  `crates/integration-tests/src/harness/` and `nix/tests/` for
  `version`/`schema_version` references and drop them).

### Out of scope

- The `ResourceBlob` schema_version inside
  `crates/shared-store/` and its load path in
  `crates/backend-backendd/src/runtime_api/invocations.rs:320-340`.
  That's F-005's runtime-side half and lives in `shared-store`;
  separate non-refactor ticket owned by the user.
- Any wire-format / NixOS-module change.

## Acceptance criteria

- [ ] `rg -n 'COLLECTION_VERSION|STATE_FILE_VERSION|ENTITY_SCHEMA_VERSION|default_client_instances_version|default_state_file_version' crates/`
      returns nothing.
- [ ] `rg -n 'version:[[:space:]]*u32' crates/backend-backendd/src/ crates/shared-protocol/src/entities.rs`
      returns nothing.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.
