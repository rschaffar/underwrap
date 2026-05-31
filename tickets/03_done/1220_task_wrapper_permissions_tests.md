# Task: Cover edit-cell modes and `clientInstancesForStore`

## Problem

Findings F-06, F-07 in
`tickets/_review/wrapper-permissions-ui_2026-05-31.md`:

- `dialogs/edit-cell.test.tsx` covers only the unsafe-debug confirmation
  flow. Untested: pre-population from an `existing` permission, the
  "Remove permission" path (`deleteWrapperPermission` is never mocked),
  `writable` toggling, saving `locked`/`unlocked`, and the
  `Number.isNaN(until)` deadline-validation branch.
- `derivations.ts:39-50` — `clientInstancesForStore` (active instances +
  stale-permission columns) is untested, including the "stale rows from a
  previous store binding" edge case its JSDoc calls out.

## What to do

1. Extend `edit-cell.test.tsx` with: `existing` pre-fill; the remove path
   (mock `deleteWrapperPermission`); saving `locked` and `unlocked`; and an
   invalid/empty deadline on `unlocked-until`.
2. Add `clientInstancesForStore` unit cases: active-only columns,
   stale-only columns, union ordering, and the empty-store case.

## Acceptance

- `cd web && npm test` (with the new tests)
- `cd web && npm run build`
