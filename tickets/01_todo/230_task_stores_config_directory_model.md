# Task: Stores config directory model (declarative + dynamic union)

Parent: `230_story_stores_management_declarative_and_dynamic.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Replace the single `stores: { <name>: StorageConfig }` map in `BackendConfig` with two config paths:
  - `stores_declarative_dir` (default `/etc/underwrap-backend/stores.d/`)
  - `stores_dynamic_dir` (default `/var/lib/underwrap-backend/stores.d/`)
- Each store definition file is `<store-name>.yaml` and contains `StorageConfig` plus optional metadata (`description`, `created_at`).
- At startup, backend enumerates both directories, parses each file, builds the union. Duplicate names → fatal error with a clear message naming both source files.
- The model tags each store in memory with `source: declarative | dynamic` for downstream gating.
- Live reload not required in v1; restart-to-apply is fine. Document this in the ticket close note.
- Decision: the existing inline `stores:` block in YAML is **removed**, not transitionally supported. This is greenfield, no backward compatibility required.

## Key files likely involved

- `crates/backend-backendd/src/server.rs`
- `crates/shared-protocol/src/config.rs`

## Acceptance criteria

- [ ] Startup unions both directories.
- [ ] Duplicate names produce a clear error mentioning both files.
- [ ] Per-store `source` tag available to handlers.
- [ ] Tests cover: only-declarative, only-dynamic, both, duplicate-conflict, malformed file rejected with a clear error, missing-directory tolerated when empty.
- [ ] Old inline `stores:` config rejected at startup with a migration hint message.
