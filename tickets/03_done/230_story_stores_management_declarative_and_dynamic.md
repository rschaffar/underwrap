# Story: Stores management (declarative + dynamic)

Parent: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Background

Stores currently live inline in the backendd YAML config. We want operators to be able to create, edit, and delete stores from the UI, while keeping NixOS declarative support for stores that should be reproducible.

## Desired design

- Backend reads store definitions from a union of two directories:
  - `/etc/underwrap-backend/stores.d/` — **declarative** source. Each file is a symlink into `/nix/store`, populated by the NixOS module.
  - `/var/lib/underwrap-backend/stores.d/` — **dynamic** source. Files written and managed by the backend itself in response to admin UI actions.
- Duplicate names across the two directories → fatal startup error.
- Admin API exposes CRUD over store definitions but refuses to mutate ones whose source is declarative.
- The Stores page in the SPA lists both kinds, badges them, and disables editing controls for declarative ones with a tooltip explaining where the definition lives.

## Tasks

1. `230_task_stores_config_directory_model.md`
2. `231_task_stores_admin_api_crud.md`
3. `232_task_nixos_module_declarative_stores.md`
4. `233_task_web_stores_page_list_and_dynamic_crud.md`
5. `234_task_encrypted_store_key_generation_flow.md`
6. `235_task_stores_test_coverage.md`

## Acceptance criteria

- [ ] Backend reads stores from both directories and unions them.
- [ ] Declarative stores are immutable via the API.
- [ ] Dynamic stores are CRUD-able via the API.
- [ ] SPA Stores page distinguishes the two kinds clearly.
- [ ] Encrypted-store key reveal-once flow exists.
- [ ] Full test coverage at every layer (Rust integration, Vitest, Playwright).
