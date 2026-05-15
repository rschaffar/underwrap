# underwrap-admin-ui

This app is the supported operator console for one underwrap backend.

It is a Next.js server-side BFF:

- browser talks only to this app
- backend bearer tokens stay server-side
- operator auth uses Argon2 password hashes + signed `HttpOnly` sessions
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

The runtime YAML is single-backend:

```yaml
session_key_file: /run/keys/underwrap-admin-ui.session

operators:
  alice:
    password_hash_file: /etc/underwrap-admin-ui/alice.phc

backend:
  label: shame01
  admin_url: http://127.0.0.1:19443
  token_file: /run/keys/underwrap-admin-backend
```

`operators` may be omitted for first-use bootstrap. In that case the UI stores
the first created operator hash in local state under
`UNDERWRAP_ADMIN_UI_STATE_DIR`.

Operator-facing routes are store-scoped:

- `/`
- `/s/[store]/resources`
- `/s/[store]/resources/[...resource]`
- `/s/[store]/wrappers`
- `/s/[store]/audit`

For NixOS deployments, use `nix/modules/admin-ui-nextjs.nix` through the flake's
`nixosModules.admin-ui` export.
