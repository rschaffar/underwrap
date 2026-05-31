# Task: Add tests for secrets dialogs, page branches, and grouping

## Problem

Finding F-06 in `tickets/_review/secrets-libsecret-ui_2026-05-31.md`.

Only `aliases-panel.test.tsx` and `tree.test.tsx` exist in the secrets
subtree. There is no coverage for the six KV dialogs, the six libsecret
dialogs, `page.tsx`, or the pure `grouping.ts` helpers.

## What to do

Add sibling `*.test.tsx` / `*.test.ts` files covering at least:

1. `grouping.ts` — `groupSecretEntries` and `resourceCount` (pure
   functions, no JSX).
2. `CreateResourceDialog` — duplicate-name rejection raises a field error
   with no network call (same for `CreateCollectionDialog`).
3. `DeleteResourceDialog` — renders the blockers alert when
   `result.ok === false`.
4. `WriteSecretDialog` — create vs edit mode and the key-rename path
   (`postSecretRename`).
5. `SecretsPage` — store-selector and empty-state branches.

## Acceptance

- `cd web && npm test` (with the new tests)
- `cd web && npm run build`
