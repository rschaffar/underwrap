# Task: Auth guard — reject `{ username: null }` as unauthenticated

## Problem

Finding F-05 (must-fix) in
`tickets/_review/shared-infra-ui_2026-05-31.md`.

`router.tsx:103` guards with `if (principal.username !== undefined)`. The
whoami schema is `username?: string | null` (`schema.ts:1183`). If the
backend ever serialises `{ username: null }` for an unauthenticated
response, the guard admits the user.

## What to do

1. Change the guard at `router.tsx:103` to `if (principal.username != null)`
   (loose inequality — catches both `null` and `undefined`).

## Acceptance

- `cd web && npm test`
- `cd web && npm run build`
- `cd web && npm run test:e2e` (auth navigation path)

## Note

This touches the SPA auth gate. The backend intends to **omit** the field
when unauthenticated, so no legitimate authenticated response changes; the
fix only closes the `null` artefact.
