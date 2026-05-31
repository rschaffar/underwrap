# Refinement review: admin SPA — wrapper-permissions page

Date: 2026-05-31
Reviewer session: claude-opus-4-8[1m]
Scope: The `wrapper-permissions` page subtree of the admin SPA —
`page.tsx`, `table.tsx`, `derivations.ts`, and the three dialogs
(`edit-cell`, `create-pair`, `delete-pair`), plus the sibling test files
(`wrapper-permissions.test.tsx`, `dialogs/edit-cell.test.tsx`). This is the
core authorization-matrix UI: it renders the per-store
`(client_instance, wrapper, resource)` permission grid and the four modes
(`Locked`, `Unlocked`, `UnlockedUntil`, `UnsafeDebugUntil`). Reviewed
read-only.

## Summary

- 10 findings across 5 categories (Simplification, Duplication,
  Accessibility/Error handling, Naming, Test coverage).
- 0 must-fix, 10 nice-to-have.
- Proposed ticket shape: **flat task tickets** (4 tasks, all small).
- The page is clean on greenfield axes: no ticket refs, no compat shims, no
  swallowed errors, and all four mode kinds are named/rendered correctly.
- Every finding becomes a ticket. No deferral bucket.
- **Verification pass (claude-opus-4-8):** F-01 confirmed — `sortPairs`
  (`derivations.ts:53`) has exactly one consumer, `page.tsx:58`, whose result
  is fed only to the `length` guard and `groupPairsByResource` (which
  re-sorts), so the pre-sort is dead work. F-03's cast shares the
  `useFormDialog` root cause (shared-infra F-20). Findings stand as written.

Cross-component note: F-08 (the `ConfirmDangerDialog` PascalCase filename) is
a shared-`components/` concern reported identically in the **shared-infra**
review (its F-02/F-03). The rename + `open`-prop normalization is owned by
the shared-infra ticket; the import update at `edit-cell.tsx:6` is folded
into that single rename so it stays atomic. It is listed here only for
traceability and is **not** double-counted as a wrapper-permissions ticket.
F-03's redundant `as Values` cast shares the `useFormDialog` root cause with
the shared-infra hook-typing fix and is sequenced after it.

## Findings

### F-01 — `sortPairs` pre-sort is dead work before `groupPairsByResource`
- **Category:** Simplification
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `page.tsx:57-60`, 178, 185; `derivations.ts:53-83`
- **Observation:** `pairRecords = sortPairs(...)` is used only for a
  `length === 0` guard and as input to `groupPairsByResource`, which fully
  re-sorts buckets and members — discarding the pre-sort. Wasted allocation
  per `view.data` change.
- **Recommendation:** Drop `sortPairs` from `page.tsx`; feed
  `view.data?.pairs ?? []` straight to `groupPairsByResource` and check
  `.length` on it. Keep `sortPairs` only if a test or other consumer needs a
  flat sorted list; otherwise unexport/remove it.
- **Behavior change?** no — render order is determined by
  `groupPairsByResource`.

### F-02 — Redundant two-branch null check in `CellContent`
- **Category:** Simplification
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `table.tsx:172-177`
- **Observation:** Both arms render `<ModeBadge mode={{ kind: "locked" }} />`
  for the no-mode case.
- **Recommendation:** Collapse to
  `return <ModeBadge mode={permission?.mode ?? { kind: "locked" }} />;`.
- **Behavior change?** no

### F-03 — Redundant `as Values` cast + defensive `?? ""` in `create-pair`
- **Category:** Simplification
- **Severity:** nice-to-have
- **Scope:** in-component (cast root cause in shared `useFormDialog`)
- **Location(s):** `dialogs/create-pair.tsx:55-56`, 61, 66
- **Observation:** `const values = v as Values` is a no-op assertion; the
  `?? ""` fallbacks on `new_resource_name` / `resource_id` guard fields the
  zod refinement has already validated for the branch.
- **Recommendation:** After the shared-infra hook-typing fix, use `v`
  directly; tighten the schema so the `"new"` branch requires a non-empty
  `new_resource_name` rather than silently sending `""`.
- **Behavior change?** no (validation already gates the path).

### F-04 — Duplicated `until` parsing block in `onSave`
- **Category:** Duplication / missed reuse
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `dialogs/edit-cell.tsx:124-147`
- **Observation:** The `"unlocked-until"` and `"unsafe-debug-until"` cases are
  character-identical except for the `kind` string, which is already the
  `kind` state variable.
- **Recommendation:** Merge into one branch that reuses `kind` (TS narrows it
  to the two `*-until` kinds), parses `untilLocal`, and builds
  `{ kind, until_epoch }`.
- **Behavior change?** no

### F-05 — `<fieldset>` mode radio group has no `<legend>`
- **Category:** Error handling / accessibility
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `dialogs/edit-cell.tsx:178-224`
- **Observation:** The mode radio group is a `<fieldset>` with only a
  `data-testid`; screen readers have no accessible label for the group.
- **Recommendation:** Add `<legend className="sr-only">Mode</legend>` (or
  `aria-labelledby`).
- **Behavior change?** no (a11y only)

### F-06 — `edit-cell.test.tsx` covers only the unsafe-debug confirm flow
- **Category:** Test layout & coverage
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `dialogs/edit-cell.test.tsx:1-84`
- **Observation:** Untested: pre-population from `existing`, the
  "Remove permission" path (`deleteWrapperPermission` is never mocked),
  `writable` toggling, saving `locked`/`unlocked`, and the
  `Number.isNaN(until)` deadline-validation branch.
- **Recommendation:** Add a second describe block for (a) `existing`
  pre-fill, (b) remove path with a mock, (c) saving locked/unlocked,
  (d) invalid deadline.
- **Behavior change?** no

### F-07 — `clientInstancesForStore` has no test
- **Category:** Test layout & coverage
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `wrapper-permissions.test.tsx:16-88`;
  `derivations.ts:39-50`
- **Observation:** The union logic (active instances + stale-permission
  columns) — exactly the edge case its JSDoc calls out — is untested.
- **Recommendation:** Add cases for active-only, stale-only, union ordering,
  and empty-store columns.
- **Behavior change?** no

### F-08 — `ConfirmDangerDialog` PascalCase filename breaks the kebab convention
- **Category:** Naming & terminology
- **Severity:** nice-to-have
- **Scope:** project-wide (owned by shared-infra ticket)
- **Location(s):** `web/src/components/ConfirmDangerDialog.tsx`;
  import at `dialogs/edit-cell.tsx:6`
- **Observation:** Every other component is kebab-case; this file and its test
  are PascalCase, creating a case-sensitive import inconsistency on Linux.
- **Recommendation:** Rename to `confirm-danger-dialog.tsx` (+ test) and
  update the import. Handled by the shared-infra rename ticket so all
  references move atomically.
- **Behavior change?** no

### F-09 — `create-pair` mode radio HTML `name` differs from the RHF field name
- **Category:** Complexity / UX
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `dialogs/create-pair.tsx:102-131`
- **Observation:** Radios use HTML `name="resource-mode"` while the RHF field
  is `"mode"`; and when `!hasExisting` the `FormItem` renders only a
  `<FormLabel>Resource</FormLabel>` with no control, leaving the
  `new_resource_name` input visually orphaned below.
- **Recommendation:** Align the HTML `name` with the RHF field (or drop it,
  since RHF drives `onChange`); when `!hasExisting`, omit the empty mode
  `FormItem` or add inline explanatory text.
- **Behavior change?** no functional change.

### F-10 — `groupPairsByResource` called inline during render
- **Category:** Simplification
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `page.tsx:185`
- **Observation:** The sort+bucket pass re-runs on every page re-render (e.g.
  dialog state changes), not just on data change.
- **Recommendation:** Wrap in
  `const groups = useMemo(() => groupPairsByResource(view.data?.pairs ?? []), [view.data])`;
  this also absorbs the now-redundant `pairRecords` intermediary from F-01.
- **Behavior change?** no

## Ticket plan

Flat tasks, number block 1200. 1200 is sequenced after the shared-infra
`useFormDialog` typing fix (for the F-03 cast); the rest are independent.

### 1200_task_wrapper_permissions_simplification.md
- **Title:** Simplify wrapper-permissions derivations, table, and create-pair
- **Description:** Remove the redundant `sortPairs` pre-sort and memoize
  `groupPairsByResource` (F-01, F-10); collapse `CellContent` (F-02); merge
  the duplicated `until` parse branches in `edit-cell` (F-04); drop the
  `as Values` cast and tighten the create-pair schema (F-03). Final
  category-7a re-read of each touched file before closing.
- **Findings:** F-01, F-02, F-03, F-04, F-10
- **Sequencing:** depends on shared-infra `useFormDialog` typing fix for F-03.
- **Acceptance:** `cd web && npm test` and `cd web && npm run build` green.

### 1210_task_wrapper_permissions_a11y_and_ux.md
- **Title:** Fix mode-radio accessibility and create-pair form semantics
- **Description:** Add a `<legend>`/`aria-labelledby` to the mode fieldset
  (F-05); align the create-pair radio HTML `name` with the RHF field and
  tidy the empty `Resource` `FormItem` when no existing resources are
  available (F-09).
- **Findings:** F-05, F-09
- **Acceptance:** `cd web && npm test` and `cd web && npm run build` green.

### 1220_task_wrapper_permissions_tests.md
- **Title:** Cover edit-cell modes and `clientInstancesForStore`
- **Description:** Extend `edit-cell.test.tsx` with the existing-permission
  pre-fill, remove path (mock `deleteWrapperPermission`), locked/unlocked
  saves, and invalid-deadline branch (F-06); add `clientInstancesForStore`
  unit cases for active-only, stale-only, union ordering, and empty store
  (F-07).
- **Findings:** F-06, F-07
- **Acceptance:** `cd web && npm test` green with the new tests.

Note: F-08 is resolved by the shared-infra `ConfirmDangerDialog` rename
ticket; no separate ticket here.

Approve the plan and start a fresh session — it will pick this report up from
`tickets/_review/` and create the tickets, then execute them via
`underwrap-ticket-workflow`.
