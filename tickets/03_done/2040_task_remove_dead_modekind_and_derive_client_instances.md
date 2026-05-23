# Task: Drop dead helpers `modeKind` and `deriveClientInstances`

## Background

Two exports in `web/src/pages/wrapper-permissions/derivations.ts`
are kept alive only by their tests:

1. **`modeKind(mode)`** returns `mode.kind` — a tautology.
   Production code uses `existing?.mode?.kind` directly
   (`dialogs/edit-cell.tsx:48`).
2. **`deriveClientInstances(permissions)`** is only called from
   `page.tsx:69-79` inside a ternary whose false branch is
   unreachable: the page short-circuits to "No stores are visible"
   when `!storeName` ten lines later (`page.tsx:108-124`).

Source review:
`tickets/_review/web_admin_spa_2026-05-23.md` F-003, F-004.

## Scope

### Delete `modeKind`

- Remove the function in
  `web/src/pages/wrapper-permissions/derivations.ts:11-13`.
- Keep `ModeKind`. Re-define it as
  `export type ModeKind = WrapperApprovalMode["kind"];` (still
  referenced by `dialogs/edit-cell.tsx:25`).
- Delete the `modeKind unwraps the tag for each variant` test in
  `web/src/pages/wrapper-permissions.test.tsx:115-120`.
- Remove the `modeKind` import in that test file.

### Delete `deriveClientInstances`

- Remove the function in
  `web/src/pages/wrapper-permissions/derivations.ts:26-30`.
- In `web/src/pages/wrapper-permissions/page.tsx:69-79`, replace
  the ternary with a direct call:

  ```ts
  const clientInstances = useMemo(
    () =>
      clientInstancesForStore(
        storeName,
        clientInstanceItems,
        view.data?.permissions ?? [],
      ),
    [storeName, clientInstanceItems, view.data],
  );
  ```

  (The early-return at `:108-124` for `!storeName` already guards
  this.)
- Delete the `deriveClientInstances dedupes and sorts` test at
  `pages/wrapper-permissions.test.tsx:63-68`.
- Remove the `deriveClientInstances` import in that test file.

## Out of scope

- Restructuring `derivations.ts` further (it still has
  `wrapperResourceKey`, `clientInstancesForStore`,
  `sortPairs`, `groupPairsByResource`).
- Changing `clientInstancesForStore` itself.

## Acceptance criteria

- [ ] `grep -rn "modeKind\b" web/src/` returns zero hits.
- [ ] `grep -rn "deriveClientInstances" web/src/` returns zero hits.
- [ ] `cd web && npm test` passes (the two tests are removed; the
      remaining `wrapper-permissions.test.tsx` cases stay green).
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes.
