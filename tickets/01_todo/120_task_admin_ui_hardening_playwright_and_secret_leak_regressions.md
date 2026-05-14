# Task: Harden the new UI and add browser end-to-end/security regression tests

Parent: `120_story_admin_ui_hardening_e2e_and_cutover.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Add browser E2E coverage for the main operator flows.
- Add regressions for auth, CSRF, reveal, mutation, wrapper mode, and matrix flows.
- Set CSP and response-cache behavior appropriate for a Next.js app.
- Verify the UI does not log secrets or expose backend tokens.

## Acceptance criteria

- [ ] Browser E2E tests cover the main operator workflows.
- [ ] Security regressions cover login/session/CSRF failures.
- [ ] Reveal responses and pages use appropriate no-store behavior.
- [ ] Backend tokens and secret values are not exposed in logs or client storage.
