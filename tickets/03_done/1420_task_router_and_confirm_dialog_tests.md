# Task: Test `safeNextPath`, the auth-guard branches, and `ConfirmDialog`

## Problem

Findings F-04 (must-fix) and F-19 in
`tickets/_review/shared-infra-ui_2026-05-31.md`:

- `safeNextPath` (`router.tsx:258`) is the only defence against
  open-redirect after sign-in. Its only caller is the login route at
  `router.tsx:195`, and there is no `router.test.*`.
- `ConfirmDialog` (`confirm-dialog.tsx`) is the primary confirm primitive
  (11 call sites) yet has no test, while `ConfirmDangerDialog` does.
- The `appLayoutRoute.beforeLoad` guard (`router.tsx:97-125`) — the only
  code on every protected navigation, including the
  `fetchWhoami → fetchBootstrapStatus` fallback and redirect logic — is
  exercised only in production.

## What to do

1. Add `router.test.ts` covering `safeNextPath`: same-origin path (pass),
   cross-origin URL (block), `..` traversal (block), non-string (block),
   `null` (null). Pure string transform; no router setup needed.
2. Add `beforeLoad` branch tests: authenticated → returns principal;
   unauthenticated + bootstrap required → redirect `/bootstrap`;
   unauthenticated + bootstrap not required → redirect `/login`.
3. Add `confirm-dialog.test.tsx`: renders title/description/body; pending
   disables the button; error renders `<Alert>`; cancel fires.

## Acceptance

- `cd web && npm test` (with the new tests) — do **not** weaken the
  open-redirect assertions to make a test pass.
- `cd web && npm run build`
