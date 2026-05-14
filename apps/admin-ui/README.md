# underwrap-admin-ui (Next.js scaffold)

This app is the new operator-console implementation for the underwrap
admin UI redesign.

## Local development

From the repo root:

```bash
cd apps/admin-ui
npm install
npm run dev
```

Then open <http://127.0.0.1:3000>.

By default the app loads `dev-fixtures/admin-ui.yaml`. In production,
set `UNDERWRAP_ADMIN_UI_CONFIG` to the real runtime config file.

## Production build check

```bash
cd apps/admin-ui
npm run build
```

## Current scope

This scaffold establishes:

- Next.js App Router + TypeScript
- dark, high-contrast ops-console shell
- multi-target / multi-store navigation placeholders
- placeholder routes for login, overview, resources, wrappers, and audit

Later tickets add:

- auth/session handling
- backend API client wiring
- reveal/upload/download flows
- wrapper and matrix mutations
- audit history data
- NixOS deployment cutover
