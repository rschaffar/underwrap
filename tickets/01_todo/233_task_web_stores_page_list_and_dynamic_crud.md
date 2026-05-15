# Task: Stores page in the SPA (list + dynamic CRUD)

Parent: `230_story_stores_management_declarative_and_dynamic.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- `/stores` route in the SPA, admin-only (nav item hidden for non-admin sessions).
- Table on desktop / cards on mobile, showing: name, source badge (`Declarative` / `Dynamic`), backend type, dir, description, created_at, secret count.
- Create-store dialog: backend type selector (`plain` / `encrypted`), dir input (validated against backend-side root), description, optional encrypted-key behavior (delegated to ticket 234).
- Edit-store dialog: description, key rotation button for encrypted (ticket 234).
- Delete confirmation dialog with `force` opt-in if non-empty; admin must type the store name to confirm destructive actions.
- Declarative-store rows show edit/delete buttons disabled with a tooltip linking to docs about the NixOS option.

## Acceptance criteria

- [ ] All listed flows work end-to-end on desktop and mobile.
- [ ] Declarative rows visibly distinguishable; edit/delete disabled with explanatory tooltip.
- [ ] Vitest tests cover form validation and the disabled-state derivation (`isDeclarative` → disabled controls).
- [ ] Playwright covers: create dynamic store, edit description, delete with and without force, declarative shows disabled controls.
- [ ] Type-to-confirm pattern enforced for delete; Playwright covers the confirm gate.
