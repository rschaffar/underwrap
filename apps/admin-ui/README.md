# underwrap-admin-ui

This app is the supported operator console for underwrap.

It is a Next.js server-side BFF:

- browser talks only to this app
- backend bearer tokens stay server-side
- operator auth uses Argon2 password hashes + signed HttpOnly sessions
- state-changing routes enforce same-origin + CSRF checks

## Local development

```bash
cd apps/admin-ui
npm install
npm run dev
```

Then open <http://127.0.0.1:3000>.

By default the app loads `dev-fixtures/admin-ui.yaml`. In production, set
`UNDERWRAP_ADMIN_UI_CONFIG` to the real runtime config file.

Local fixture credentials:

- username: `alice`
- password: `hunter2`

## Checks

```bash
cd apps/admin-ui
npm test
npm run build
npm run test:e2e
```

`npm run test:e2e` builds the app, starts a local browser-driven test stack,
and runs Playwright regressions against the Next.js UI.

## Runtime model

The app binds through the normal Next.js `HOSTNAME` / `PORT` environment
variables and reads backend/operator configuration from
`UNDERWRAP_ADMIN_UI_CONFIG`.

For NixOS deployments, use `nix/modules/admin-ui-nextjs.nix` through the flake's
`nixosModules.admin-ui` export.
