# Task: Fix HTTP same-origin browser form submissions when Origin is null

Parent: `120_story_admin_ui_hardening_e2e_and_cutover.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Fix the admin UI so plain-HTTP same-origin browser form submissions do not fail just because the browser sends `Origin: null`.
- Keep same-origin protections in place.
- Add regression coverage for the browser/header combination seen on the live `shame01` deployment.

## Acceptance criteria

- [ ] The login/bootstrap flow works for same-origin browser form submissions even when the browser sends `Origin: null`.
- [ ] Same-origin protections still reject cross-origin requests.
- [ ] Tests cover the `Origin: null` + same-origin `Referer` case.
