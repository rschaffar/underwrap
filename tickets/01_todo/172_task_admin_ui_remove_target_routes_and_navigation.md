# Task: Remove target-based routes and flatten the admin UI navigation

Parent: `170_story_admin_ui_single_backend_simplification.md`

## Scope

- Remove the `/t/[target]/...` route hierarchy from the Next.js app.
- Flatten pages so the store becomes the primary scope, e.g. `/s/[store]/...`.
- Remove the target switcher from the shell.
- Keep overview, resources, wrappers, and audit pages working after the route change.
- Update action routes/redirects/forms to the new path scheme.

## Key files likely involved

- `apps/admin-ui/components/shell.tsx`
- `apps/admin-ui/app/page.tsx`
- `apps/admin-ui/app/t/[target]/...` (delete/replace)
- resource / wrapper / audit page components and redirect helpers
- action route helpers in `apps/admin-ui/lib/server/*`

## Handoff notes

This is a greenfield cleanup, so backward compatibility with the old `/t/...`
URL scheme is not required unless explicitly requested.

A simple redirect shim could be added temporarily if useful during migration,
but the intended final design is a target-free operator UI.

## Acceptance criteria

- [ ] Target switcher is gone.
- [ ] Pages no longer require a target path segment.
- [ ] Form actions and redirects follow the new route structure.
- [ ] Browser regressions are updated for the flattened URL scheme.
