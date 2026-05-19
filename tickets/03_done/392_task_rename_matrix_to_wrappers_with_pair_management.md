# Task: Rename Matrix → Wrappers; add pair management

Parent: `390_story_spa_wrappers_page_and_secrets_resource_management.md`

## Scope

Rename the existing Matrix page and turn it into the management surface
for wrapper/resource pairs:

- File rename: `web/src/pages/matrix.tsx` → `web/src/pages/wrappers.tsx`
  (and its `.test.tsx`).
- Route rename: `/matrix` → `/wrappers` in
  `web/src/router.tsx`. No redirect from the old route — greenfield.
- App-shell nav label: `Matrix` → `Wrappers`
  (`web/src/components/app-shell.tsx`).
- Document title: `underwrap — wrappers`.
- Replace the `deriveWrapperResources` catalog-driven derivation with
  a direct `postPairsList` query. Empty state copy updated to mention
  "create a wrapper/resource pair".
- Add a **Create pair** dialog: pick a resource from a dropdown
  (populated by `postResourcesList`, with an inline "create resource"
  shortcut), type a wrapper name, submit. Surface refusal reasons from
  the backend verbatim (`PairAlreadyExists`, `ResourceNotFound`).
- Add a **Delete pair** action on each row. The delete dialog follows
  the new simple confirm/cancel pattern (no type-to-confirm). Refusal
  with grant dependents surfaces a clear list of blocking
  `(client_instance, mode)` rows.
- Show provenance on each row: `created_by` + relative timestamp,
  tooltip with the full ISO timestamp.

Grants in cells continue to work exactly as today — this task is only
about pair-record management plus the cosmetic + structural rename.

## Acceptance criteria

- [ ] No file or route named `matrix` remains.
- [ ] Operator can create and delete a pair from the Wrappers page;
      both refusal paths render the backend's diagnostic shape.
- [ ] Provenance visible on each row.
- [ ] Existing grant edit affordances still work.
- [ ] Vitest unit tests for the renamed page green (rewritten where
      they used catalog mocks).

## Out of scope

- E2E coverage and SPA test rewrites — covered by 395.
- Backend changes — already shipped under story 380.
