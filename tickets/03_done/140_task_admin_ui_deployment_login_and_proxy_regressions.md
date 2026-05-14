# Task: Add deployment login, proxy-origin, and service-start regressions for the Next.js admin UI

Parent: `120_story_admin_ui_hardening_e2e_and_cutover.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Add tests that cover the deployed Next.js admin UI path more directly.
- Catch production/runtime login failures that unit tests and dev-server browser tests miss.
- Catch same-origin/proxy header regressions on `/auth/login`.
- Catch service-level startup regressions for the NixOS/systemd deployment path.

## Why this follow-up exists

After the main cutover landed, deployment on `shame01` exposed gaps in coverage:

- the old Rust binary path in the host config was not exercised by repo tests
- `MemoryDenyWriteExecute = true` broke Node/V8 under systemd hardening
- proxied HTTPS-style login requests can hit same-origin/forwarded-header mismatches
- successful login on the packaged server/runtime path was not directly covered

## Acceptance criteria

- [ ] Tests cover successful and failed login on the packaged/runtime server path.
- [ ] Tests cover proxied same-origin expectations for `/auth/login`.
- [ ] Tests cover the deployed service startup path strongly enough to catch Node/V8 hardening incompatibilities.
- [ ] The new tests run in the project's normal validation flow for the touched area.
