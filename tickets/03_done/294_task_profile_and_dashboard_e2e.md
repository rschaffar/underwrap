# Task: Playwright e2e coverage for Profile and Dashboard pages

Parent: `290_story_admin_spa_post_rework_polish.md`

## Background

`/profile` and the dashboard landing page (`/`) have no Playwright
coverage today. Both are simple pages, but they sit on the critical
session-management surface (password change, current-user display,
logged-in dashboard render). Cutover bugs in these pages would
silently degrade the operator experience without breaking any of
the existing flows.

## Scope

### Profile

Add a spec (in a new `web/e2e/profile.spec.ts` or as part of
`auth-flows.spec.ts` — pick whichever keeps fixtures simple) that:

- Logs in as a non-admin operator.
- Visits `/profile`, asserts the displayed identity matches the
  logged-in operator.
- Changes the password through the self-service form.
- Logs out, logs back in with the new password.

### Dashboard

Add at least a golden-path render check that:

- Logs in as an admin operator.
- Visits `/` (or whatever the dashboard route is registered as).
- Asserts the page renders without an error boundary and shows at
  least one expected widget / metric (pick a stable element — a
  heading or a fetched-data card).

The dashboard test does NOT need to assert specific data values;
it's a render smoke test that catches "this page crashes after a
backend response shape change."

## Acceptance criteria

- [ ] Both flows run under `npm run test:e2e`.
- [ ] The profile spec exercises the password-change → re-login
      round trip against the real backend.
- [ ] The dashboard spec fails when the route's error boundary is
      tripped (verify by temporarily breaking a backend response and
      confirming the test goes red).

## Out of scope

- Visual regression / pixel diffing.
- Asserting specific dashboard metric values.
