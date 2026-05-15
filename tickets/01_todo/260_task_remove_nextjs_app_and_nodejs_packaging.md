# Task: Remove the Next.js app and Node-based packaging

Parent: `260_story_cutover_and_documentation.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Delete `apps/admin-ui/` entirely.
- Delete `nix/modules/admin-ui-nextjs.nix`.
- Remove `underwrap-admin-ui-nextjs` and `underwrap-admin-ui` flake outputs.
- Remove `services.underwrap-admin-ui` references from `nix/modules/underwrap.nix`.
- Remove `nixosModules.admin-ui` re-export.
- Remove the operator-records migration CLI subcommand added in ticket 210 if it's no longer needed.

## Acceptance criteria

- [ ] No references to Next.js, `apps/admin-ui`, or `nodejs` remain in `flake.nix`, `nix/`, or the workspace `Cargo.toml`.
- [ ] `cargo build --workspace` and `nix flake check` are clean.
- [ ] VM test still passes (it should already exercise the new path post-262).
- [ ] The change is a single ticket-shaped commit per the workflow.
