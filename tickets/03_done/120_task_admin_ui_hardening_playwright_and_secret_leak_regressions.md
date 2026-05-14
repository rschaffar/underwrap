# Task: Harden the new UI and add browser end-to-end/security regression tests

Parent: `120_story_admin_ui_hardening_e2e_and_cutover.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Add browser E2E coverage for the main operator flows.
- Add regressions for auth, CSRF, reveal, binary upload/download, mutation, wrapper mode, matrix, and audit-history flows.
- Set CSP and response-cache behavior appropriate for a Next.js app.
- Verify the UI does not log secrets or expose backend tokens.

## Handoff notes / current repo state

Checks already used successfully during implementation so far:

- `cd apps/admin-ui && npm test && npm run build`
- `nix build .#underwrap-admin-ui-nextjs --no-link`
- backend endpoint tickets also ran `cargo test -p backend-backendd`

Current frontend tests are lightweight Node tests in:

- `apps/admin-ui/tests/auth.test.ts`
- `apps/admin-ui/tests/csrf.test.ts`

This ticket should add the browser/E2E layer on top, not replace those lower-level tests.

Important flake/Nix gotcha to remember while adding new frontend test files:

- if a new file is not git-tracked/staged, the flake source snapshot used by `nix build` will not include it
- if Nix cannot find a just-created frontend file but local `npm run build` can, check git tracking first

## Acceptance criteria

- [ ] Browser E2E tests cover the main operator workflows.
- [ ] Security regressions cover login/session/CSRF failures.
- [ ] Reveal and download responses/pages use appropriate no-store behavior.
- [ ] Backend tokens and secret values are not exposed in logs or client storage.
- [ ] Audit-history flows and reveal/download audit events are covered by regression tests.
