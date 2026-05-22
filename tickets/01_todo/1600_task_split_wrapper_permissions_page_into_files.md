# Task: Split `pages/wrapper-permissions.tsx` into per-component files

Parent: `1500_story_web_spa_refinement.md`
Resolves: F-002 (review report `web_spa_2026-05-22.md`). F-026
  already landed in ticket 1570.

## Background

`web/src/pages/wrapper-permissions.tsx` is 1139 LoC and hosts:

- `WrapperPermissionsPage`
- Derivation helpers (exported, consumed by tests):
  `ModeKind`, `modeKind`, `WrapperResource`,
  `wrapperResourceKey`, `formatRelativeFromEpoch` (now in
  `lib/epoch.ts` after 1510), `deriveClientInstances`,
  `clientInstancesForStore`, `sortPairs`,
  `groupPairsByResource`
- Grid + badge components: `ResourceGroupedPairsTable`,
  `CellContent`, `ModeBadge`
- Dialogs: `CreatePairDialog`, `DeletePairDialog`,
  `EditCellDialog`
- `toDatetimeLocal` (moved to `lib/epoch.ts` in 1510)
- `describeCreator` (moved to `lib/creator.ts` in 1510)

After tickets 1510, 1520, 1530, 1540, 1560, and 1570 land, this
file is already smaller and uses shared building blocks; the
remaining size is structural — every dialog inline.

## Scope

Restructure as:

```
web/src/pages/wrapper-permissions/
  page.tsx              // WrapperPermissionsPage
  derivations.ts        // ModeKind, modeKind, WrapperResource,
                        // wrapperResourceKey,
                        // deriveClientInstances,
                        // clientInstancesForStore, sortPairs,
                        // groupPairsByResource
  table.tsx             // ResourceGroupedPairsTable +
                        // CellContent + ModeBadge
  dialogs/
    create-pair.tsx     // CreatePairDialog
    delete-pair.tsx     // DeletePairDialog
    edit-cell.tsx       // EditCellDialog (already cleaned up
                        // by 1570 — no casts)
```

- Re-point the router import in `web/src/router.tsx:17` from
  `@/pages/wrapper-permissions` to
  `@/pages/wrapper-permissions/page`.
- Re-point `web/src/pages/wrapper-permissions.test.tsx:4-12`:
  - `ModeBadge` from `./wrapper-permissions/table`.
  - `deriveClientInstances`, `groupPairsByResource`,
    `modeKind`, `sortPairs`, `wrapperResourceKey` from
    `./wrapper-permissions/derivations`.
  - `formatRelativeFromEpoch` from `@/lib/epoch` (already
    moved in 1510, possibly under the renamed
    `formatEpochRelative` if that decision was taken in 1510).
- `web/src/components/policy-viewer-dialog.tsx` already lives
  outside this page; no change there. Its `import …` line for
  `postPolicyGet` stays the same.
- Verify the table's row-spanning logic (the
  resource-grouped table at lines 491-578 of the current file)
  moves cleanly with the right testids; the e2e suite at
  `web/e2e/wrappers.spec.ts` exercises specific testids like
  `wrappers-cell-<wrapper>-<resource>-<client>`.

## Acceptance criteria

- [ ] `web/src/pages/wrapper-permissions.tsx` is gone; the
      directory layout above exists.
- [ ] `page.tsx` does not exceed 300 LoC.
- [ ] `table.tsx` does not exceed 300 LoC.
- [ ] No dialog file exceeds 250 LoC.
- [ ] `cd web && npm test` passes (the test file's imports re-point
      cleanly).
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes (`web/e2e/wrappers.spec.ts`
      and `web/e2e/wrappers.mobile.spec.ts`).
