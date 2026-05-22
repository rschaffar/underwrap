# Task: Split `pages/stores.tsx` into per-dialog files

Parent: `1500_story_web_spa_refinement.md`
Resolves: F-003 (review report `web_spa_2026-05-22.md`). Partially
  touches F-023 where dialog-prop narrowing is trivial; the rest
  of F-023 routes to a follow-up if cleanup turns noisy.

## Background

`web/src/pages/stores.tsx` is 1095 LoC and hosts:

- `StoresPage` (the page component)
- `toStoreDetail`, `storeEditDisabledTitle`, `storeDeleteTitle`
  (small helpers)
- `StoresTable` (the table)
- `CreateStoreDialog`, `EditStoreDialog`, `KeyRevealDialog`,
  `UnlockStoreDialog`, `LockStoreDialog`, `DeleteStoreDialog`
- Zod schemas inline next to their dialogs

That's far past the ~600 LoC guidance. `KeyRevealDialog` is
opened by both create (post-success) and edit (post-rotate-key),
so it must end up in a shared location.

## Scope

Restructure as:

```
web/src/pages/stores/
  page.tsx          // StoresPage + the three small helpers
  table.tsx         // StoresTable + StoresTableProps
  dialogs/
    create.tsx      // CreateStoreDialog + its zod schema
    edit.tsx        // EditStoreDialog + its zod schema
    unlock.tsx     // UnlockStoreDialog + its zod schema
    lock.tsx       // LockStoreDialog
    delete.tsx     // DeleteStoreDialog
    key-reveal.tsx // KeyRevealDialog (used by create + edit)
```

- Re-point the router import in `web/src/router.tsx:19` from
  `@/pages/stores` to `@/pages/stores/page`.
- Re-point `web/src/pages/stores.test.tsx:6` from
  `./stores` to `./stores/page`. Tests don't reach the dialog
  internals; the shared `KeyRevealDialog` doesn't need its own
  test file beyond what stores.test exercises.
- Decision (locally): whether to keep `toStoreDetail` as a
  helper in `pages/stores/page.tsx` or to narrow each dialog's
  prop type to a hand-written `StoreSummary` so the shim
  disappears. Recommended: keep `toStoreDetail` for now;
  full narrowing is F-023's follow-up.

## Acceptance criteria

- [ ] `web/src/pages/stores.tsx` is gone; the directory layout
      above exists.
- [ ] No file under `web/src/pages/stores/` exceeds 300 LoC for
      `page.tsx`, or 250 LoC for any dialog file.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes (stores Playwright
      specs at `web/e2e/stores.spec.ts`).
