# Task: Update docs, cut over to the new app, and remove the Rust admin UI

Parent: `120_story_admin_ui_hardening_e2e_and_cutover.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Update `docs/admin-ui.md`, deployment docs, and related references.
- Document the new runtime/deployment model and operator workflows.
- Cut over packaging/deployment to the new app.
- Remove `crates/backend-admin-ui` after the replacement is ready.

## Acceptance criteria

- [ ] Docs describe the new Next.js app rather than the Rust HTML UI.
- [ ] Deployment guidance reflects the new runtime shape.
- [ ] The old Rust admin UI is removed after cutover.
- [ ] The repo has one clear supported admin UI path.
