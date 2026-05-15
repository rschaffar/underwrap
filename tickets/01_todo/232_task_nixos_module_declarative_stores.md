# Task: NixOS module declarative stores option

Parent: `230_story_stores_management_declarative_and_dynamic.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Add `services.underwrap-backend.declarativeStores.<name> = { backend = "plain" | "encrypted"; dir = ...; keyFile = ...; description = ...; }` option to the NixOS module.
- Module materializes one YAML file per declarative store into `/etc/underwrap-backend/stores.d/<name>.yaml` via `environment.etc` (so entries are symlinks into `/nix/store`).
- Module removes the old inline `stores:` block from the backendd YAML it generates; backendd reads the directory instead.
- For declarative encrypted stores, `keyFile` is a required option and points to a sops-managed file path.
- Update `nix/tests/node.nix` and `nix/tests/test.py` to use the new option.
- Document the option in `nix/modules/underwrap.nix` doc comment and add a usage example to `docs/backend.md`.

## Acceptance criteria

- [ ] Module generates one symlinked YAML per declarative store under `/etc/underwrap-backend/stores.d/`.
- [ ] Module asserts when a declarative encrypted store omits `keyFile`.
- [ ] VM test passes with the new option (no longer references the old inline `stores:` block).
- [ ] Docs updated: module doc comment, `docs/backend.md` usage example.
