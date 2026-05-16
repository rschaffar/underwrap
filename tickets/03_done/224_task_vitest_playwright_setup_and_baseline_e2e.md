# Task: Vitest + Playwright setup and baseline e2e

Parent: `220_story_vite_react_shadcn_spa_scaffold.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Vitest config for `web/`: `jsdom` env, React Testing Library, coverage threshold ≥80% for `web/src/lib/` and `web/src/api/`.
- Playwright config: spawns a `backend-backendd` binary in a temp dir with a tmp store, listens on a random TLS port using a self-signed cert; tests run against `https://localhost:<random>/`.
- Baseline e2e scenarios:
  - bootstrap-then-login happy path
  - login wrong password → inline error
  - logout invalidates session (next request to `/v1/admin/...` returns 401, UI redirects to /login)
  - CSRF token attached to POST (verified via Playwright request inspection)
  - 401 from API → redirect to `/login?next=...`
  - mobile viewport 375×812: drawer nav opens, login form fits without horizontal scroll
- Nix derivation runs `npm run build && npx vitest run`; Playwright runs as a separate `nix flake check` that spawns its own backendd.

## Acceptance criteria

- [ ] `npm run test` runs Vitest.
- [ ] `npm run test:e2e` runs Playwright against a freshly-spawned backendd.
- [ ] No global config required — each Playwright worker spawns its own backendd in a temp dir.
- [ ] All listed baseline scenarios pass.
- [ ] `nix flake check` runs both suites.
- [ ] New test files are git-tracked.
