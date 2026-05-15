# Task: Stores end-to-end and regression tests

Parent: `230_story_stores_management_declarative_and_dynamic.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Rust integration tests under `crates/integration-tests/`:
  - Union of declarative + dynamic directories
  - Duplicate-name conflict at startup
  - Declarative mutation rejection (create-over-existing, update, delete all attempted on declarative)
  - Encrypted-store key one-time reveal (subsequent reads never include the key)
  - Audit events emitted for every store mutation with operator principal recorded
- Vitest tests for Stores page derivation rules (`isDeclarative` → disabled controls; type-to-confirm gate).
- Playwright e2e:
  - Admin creates dynamic plain store
  - Admin creates dynamic encrypted store, sees key-reveal modal, key cannot be re-revealed
  - Admin tries to edit declarative store, sees disabled controls and tooltip
  - Operator (non-admin) cannot see Stores nav item or reach `/stores` directly
  - Force-delete a non-empty dynamic store after type-to-confirm

## Acceptance criteria

- [ ] All listed tests run green via `cargo test --workspace` and `nix flake check`.
- [ ] Failure modes (declarative-mutate, non-admin access) covered as negative tests.
- [ ] No flake-prone waits in Playwright; tests rely on `expect` polling and event-based assertions.
