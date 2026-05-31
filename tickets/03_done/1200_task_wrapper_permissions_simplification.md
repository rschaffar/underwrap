# Task: Simplify wrapper-permissions derivations, table, and create-pair

## Problem

Findings F-01, F-02, F-03, F-04, F-10 in
`tickets/_review/wrapper-permissions-ui_2026-05-31.md`:

- `page.tsx:57-60` pre-sorts via `sortPairs`, but the result feeds only a
  `length === 0` guard and `groupPairsByResource` (`derivations.ts:68-83`),
  which fully re-sorts — the pre-sort is dead work. (`sortPairs`,
  `derivations.ts:53`, has this as its only consumer.)
- `page.tsx:185` calls `groupPairsByResource` inline on every render.
- `table.tsx:172-177` — `CellContent` has two arms that both render
  `<ModeBadge mode={{ kind: "locked" }} />` for the no-mode case.
- `dialogs/create-pair.tsx:55-56,61,66` — redundant `as Values` cast and
  defensive `?? ""` fallbacks on fields the zod refinement already gates.

## What to do

1. Remove `sortPairs` from `page.tsx`; feed `view.data?.pairs ?? []`
   directly to `groupPairsByResource` and check `.length` on it. Wrap the
   grouping in `useMemo(..., [view.data])`. Unexport/remove `sortPairs` if
   nothing else needs a flat sorted list (keep its unit test or drop it
   with the function).
2. Collapse `CellContent` to
   `return <ModeBadge mode={permission?.mode ?? { kind: "locked" }} />;`.
3. In `create-pair.tsx`, drop the `as Values` cast and use the argument
   directly (**depends on shared-infra ticket 1430**); tighten the schema
   so the `"new"` branch requires a non-empty `new_resource_name` rather
   than silently sending `""`.
4. Merge the duplicated `until` parse branches in `edit-cell.tsx:124-147`
   into one branch reusing the `kind` state variable (TS narrows it to the
   two `*-until` kinds).
5. Final category-7a re-read of each touched file.

## Acceptance

- `cd web && npm test`
- `cd web && npm run build`
