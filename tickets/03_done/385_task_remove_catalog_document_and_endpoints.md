# Task: Remove the catalog document, endpoints, and wire types

Parent: `380_story_backend_resource_and_pair_entities.md`

## Scope

Tear out everything related to the legacy catalog after the new
register endpoint and admin CRUD have landed and the SPA has been
cut over.

Removed in this task:

- `__underwrap/catalog/document` reads and writes
  (`crates/backend-backendd/src/server.rs:216, 1218, 1285`).
- `POST /v1/catalog/put` route + `ServiceCatalogPut*` types.
- `POST /v1/admin/catalog/get` route + `AdminCatalogGet*` types.
- `CatalogEntry` wire type and the SPA's generated TS counterpart.
- The `StorageBackend::register_catalog` trait default
  (`crates/shared-store/src/lib.rs:72`) and the `ServiceBackend` impl
  (`crates/shared-store/src/lib.rs:400`).
- Any remaining `crates/backend-backendd/src/server.rs` helpers tied
  to catalog persistence (`StoredCatalogEntry`,
  `load_catalog_document`, `store_catalog_document`).
- Integration test `service_mode_registers_catalog_entries_on_startup`
  in `crates/integration-tests/src/tests/service_transport.rs` —
  replaced or rewritten against the new register endpoint as part of
  `386_task_backend_entity_model_test_coverage.md`.

Sanity check: `rg -i catalog crates/ web/src nix/` should return only
hits that are clearly unrelated (comments in docs that have been
updated explaining the rename).

## Acceptance criteria

- [ ] Zero references to "catalog" in code (case-insensitive) other
      than intentional historical mentions in tickets and changelog.
- [ ] `cargo build --workspace` and `cargo test --workspace` green.
- [ ] No store on disk needs `__underwrap/catalog/document` cleanup;
      the entry is just left orphaned (greenfield — no migration).

## Out of scope

- Doc rewrites for `docs/backend.md`, CLAUDE.md, examples — covered
  by `399_task_docs_for_resource_and_pair_entity_model.md`.
