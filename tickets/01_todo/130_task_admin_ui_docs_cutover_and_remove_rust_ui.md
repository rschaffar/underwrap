# Task: Update docs, cut over to the new app, and remove the Rust admin UI

Parent: `120_story_admin_ui_hardening_e2e_and_cutover.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Update `docs/admin-ui.md`, deployment docs, and related references.
- Document the new runtime/deployment model, first-class NixOS integration path, and operator workflows.
- Cut over packaging/deployment to the new app.
- Remove `crates/backend-admin-ui` after the replacement is ready.

## Handoff notes / current repo state

Docs and integration groundwork already present:

- redesign RFC: `docs/admin-ui-nextjs-rfc.md`
- docs index link added in `docs/index.md`
- packaged app: `apps/admin-ui/`
- flake package: `underwrap-admin-ui-nextjs`
- NixOS module: `nix/modules/admin-ui-nextjs.nix`

This ticket should finish the narrative and cutover by:

1. rewriting operator-facing docs to describe the Next.js app as the primary UI
2. documenting `UNDERWRAP_ADMIN_UI_CONFIG` and the NixOS module options
3. removing `crates/backend-admin-ui` once parity is good enough
4. removing stale references to the Rust HTML UI from docs/flake/module text where appropriate

## Acceptance criteria

- [ ] Docs describe the new Next.js app rather than the Rust HTML UI.
- [ ] Deployment guidance reflects the new runtime shape and NixOS integration path.
- [ ] The old Rust admin UI is removed after cutover.
- [ ] The repo has one clear supported admin UI path.
