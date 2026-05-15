# Task: Replace admin UI multi-target config with a single-backend config

Parent: `170_story_admin_ui_single_backend_simplification.md`

## Scope

- Change `apps/admin-ui` runtime config from:
  - `backends: { <name>: { admin_url, token_file } }`
  to:
  - `backend: { admin_url, token_file, label? }`
- Remove multi-backend loader helpers and client lists.
- Simplify backend-client/navigation code to exactly one configured backend.
- Update bootstrap/login/runtime tests for the new config shape.

## Key files likely involved

- `apps/admin-ui/lib/server/config.ts`
- `apps/admin-ui/lib/server/backend-client.ts`
- `apps/admin-ui/lib/server/navigation.ts`
- `apps/admin-ui/tests/auth.test.ts`
- `apps/admin-ui/tests/production-login.test.ts`
- `apps/admin-ui/dev-fixtures/admin-ui.yaml`
- `apps/admin-ui/dev-fixtures/e2e-admin-ui.yaml`

## Handoff notes

This task should happen before route/UI flattening because the rest of the app
still depends on the old target abstraction in loaders and navigation.

The current user-visible target label `robert` on `shame01` is one symptom of
this design bug.

## Acceptance criteria

- [ ] Config parsing no longer requires a backend map.
- [ ] Only one backend client is constructed at runtime.
- [ ] Existing tests are updated to the singular backend config shape.
