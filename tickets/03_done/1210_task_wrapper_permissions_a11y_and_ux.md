# Task: Fix mode-radio accessibility and create-pair form semantics

## Problem

Findings F-05, F-09 in
`tickets/_review/wrapper-permissions-ui_2026-05-31.md`:

- `dialogs/edit-cell.tsx:178-224` — the mode radio group is a `<fieldset>`
  with only a `data-testid`; no `<legend>`, so screen readers have no
  accessible label for the group.
- `dialogs/create-pair.tsx:102-131` — the radios use HTML
  `name="resource-mode"` while the RHF field is `"mode"`; and when
  `!hasExisting` the `FormItem` renders only a `<FormLabel>Resource</FormLabel>`
  with no control, leaving the `new_resource_name` input visually orphaned.

## What to do

1. Add `<legend className="sr-only">Mode</legend>` (or `aria-labelledby`)
   to the mode fieldset.
2. Align the create-pair radio HTML `name` with the RHF field (or drop the
   `name` since RHF drives `onChange`).
3. When `!hasExisting`, omit the empty `Resource` `FormItem`/`FormLabel`
   for the mode field or add inline explanatory text.

## Acceptance

- `cd web && npm test`
- `cd web && npm run build`
