# Task: Rename `ConfirmDangerDialog` to kebab-case and normalize `open`

## Problem

Findings F-02, F-03 in `tickets/_review/shared-infra-ui_2026-05-31.md`
(also resolves wrapper-permissions finding F-08):

- `components/ConfirmDangerDialog.tsx` (+ `.test.tsx`) is the only
  PascalCase component file; every other is kebab-case, creating a
  case-sensitive import inconsistency on Linux.
- `ConfirmDangerDialog` accepts an `open: boolean` prop and is mounted
  persistently by its single caller (`edit-cell.tsx:286`), whereas
  `ConfirmDialog` hardcodes `open` and relies on conditional mounting (11
  callers).

## What to do

1. Rename `ConfirmDangerDialog.tsx` → `confirm-danger-dialog.tsx` and its
   test accordingly; update the import at
   `pages/wrapper-permissions/dialogs/edit-cell.tsx:6`.
2. Remove the `open` prop from the component; switch its caller to
   conditional mounting (`{open ? <…/> : null}`), matching the established
   pattern.

## Acceptance

- `cd web && npm test`
- `cd web && npm run build`
