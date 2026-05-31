# Task: Split the API client into plumbing + per-domain endpoint modules

## Problem

Finding F-01 in `tickets/_review/shared-infra-ui_2026-05-31.md`.

`api/client.ts` is 585 LoC. Lines 1-203 are genuine plumbing (CSRF
middleware, `unwrap`, `api.GET/POST/PUT/DELETE`); 205-239 are schema
re-exports; 241-585 are 40+ one-line named wrappers (`fetchStores`,
`postStoreCreate`, `deleteLibsecretItem`, …) that only forward to `api.X`.
Every new endpoint touches this one file.

## What to do

1. Move the named endpoint wrappers out of `client.ts` into co-located
   per-domain modules (`api/stores.ts`, `api/secrets.ts`,
   `api/client-instances.ts`, `api/users.ts`, `api/libsecret.ts`, …; or a
   single `api/endpoints.ts` if per-domain is overkill).
2. Leave `client.ts` ≤ ~200 LoC: types + CSRF middleware + the `api`
   surface.
3. Update all importers across the SPA.

## Acceptance

- `cd web && npm run build` (tsc resolves every moved import)
- `cd web && npm test`

## Note

This is the largest mechanical move in the epic. Sequence it so it does not
collide with other in-flight edits to `api/client.ts`.
