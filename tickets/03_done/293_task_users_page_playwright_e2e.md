# Task: Playwright e2e coverage for the Users CRUD page

Parent: `290_story_admin_spa_post_rework_polish.md`

## Background

`web/src/pages/users.tsx` has vitest unit coverage with mocked API
calls, but no Playwright e2e spec exercises the page against a real
backend. The existing `web/e2e/multi-user.spec.ts` covers the
operator-grant flow end-to-end but treats Users CRUD as setup, not
as the subject under test. Other top-level pages (Stores, Matrix)
already have dedicated specs.

## Scope

Add `web/e2e/users.spec.ts` (or extend `multi-user.spec.ts` if the
fixtures align better) covering at minimum:

- Create a new operator (filling the dialog, accepting the
  one-time-password reveal).
- Edit an existing operator (display name change, role change).
- Delete an operator that is not the last admin.
- The last-admin-protection guard refuses to delete the only
  operator with the admin role and surfaces the error in the UI.
- Self-service password change via `/profile` — or note that this
  is covered by ticket 294 and skip here.

Tests must use the existing Playwright harness (real backend, real
session cookie) — no API mocking. Run under both the desktop and
mobile Playwright projects (the existing matrix runs the spec
twice; new specs should opt into the same).

## Acceptance criteria

- [ ] `cd web && npm run test:e2e` runs the new spec and it passes
      on desktop, tablet, and mobile projects.
- [ ] Each acceptance scenario above corresponds to a named test in
      the spec.
- [ ] The spec leaves the backend in a clean state (test users
      created during the run are deleted at the end, or use a fresh
      backend per test as the existing specs do).

## Out of scope

- Permission-matrix corner cases that `multi-user.spec.ts` already
  covers — don't duplicate.
