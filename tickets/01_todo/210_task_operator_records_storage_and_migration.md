# Task: Move operator records into the backend store

Parent: `210_story_rust_native_auth_sessions_multiuser.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Define an `OperatorRecord` DTO in `crates/shared-protocol` (or a new module in `backend-backendd`) with:
  - `id` (uuid)
  - `username` (lowercase, trimmed, unique)
  - `password_hash` (Argon2id PHC string)
  - `role` (enum: `admin` | `operator`)
  - `granted_stores: Vec<String>` (ignored for `admin` role)
  - `created_at` (epoch seconds)
  - `password_changed_at` (epoch seconds)
- Store the operator collection as a single JSON blob under reserved secret id `__underwrap/admin/operators` in the backend's chosen admin-home store (configurable in `AdminConfig`; defaults to the first declared store).
- Annotate DTO with serde shapes consistent with the eventual ts-rs export in ticket 222 (snake_case fields, tagged enums where serde defaults are ambiguous). Add a TODO marking the spot ts-rs derive will land.
- Add a migration helper (subcommand `underwrap-backendd admin import-operators-from-yaml`) that reads the legacy `apps/admin-ui/dev-fixtures/admin-ui.yaml`-shape config + `operators.json` state and writes equivalent `OperatorRecord`s. Helper is removed in ticket 260.

## Key files likely involved

- `crates/shared-protocol/src/lib.rs` and/or a new module
- `crates/backend-backendd/src/server.rs`
- New module `crates/backend-backendd/src/operators.rs`
- New `crates/backend-backendd/src/admin_cli.rs` for the migration subcommand

## Acceptance criteria

- [ ] `OperatorRecord` serializes/deserializes round-trip via serde.
- [ ] Operator collection persists across backend restarts.
- [ ] Migration helper reads a legacy Next.js operator config + state file and writes the resulting records into the chosen store.
- [ ] Unit tests cover: round-trip serialization, duplicate-username rejection, case-insensitive uniqueness, empty-collection bootstrap signal, migration helper happy path and conflict.
- [ ] Password hash field is never logged (verified by a `tracing` subscriber test capturing log output during operator CRUD).
