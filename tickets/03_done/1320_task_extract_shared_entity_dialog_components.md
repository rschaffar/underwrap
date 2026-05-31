# Task: Extract shared StatusChip, Textarea, and AutoCreateField components

## Problem

Findings F-08, F-10, F-11 in
`tickets/_review/stores-client-instances-ui_2026-05-31.md`:

- `stores/table.tsx:73-94` renders status chips inline with hardcoded
  Tailwind (`rounded-full`); `client-instances/badges.tsx:1-36` extracts
  `StatusBadge`/`AutoCreateBadge` (`rounded-md`). Same visual concept, two
  radius tokens, no shared component.
- `client-instances/dialogs/create.tsx:148-178` and `edit.tsx:158-184` —
  the auto-create checkbox + label + description block is copy-pasted.
- `client-instances/dialogs/create.tsx:136-145` and `edit.tsx:139-144` —
  an identical cert-PEM `<textarea>` is duplicated; the project has no
  shadcn `Textarea` wrapper.

## What to do

1. Extract a shared `StatusChip` used by both the store status badges and
   the client-instance status badges, with a single border-radius token.
2. Add `web/src/components/ui/textarea.tsx` (mirroring the `Input`
   wrapper) and use it in both client-instance dialogs.
3. Extract an `AutoCreateField` component taking `control`, `testId`, and
   an optional extended description; use it in create and edit.

## Acceptance

- `cd web && npm test` (no visual regression in the stores/client-instances
  tests)
- `cd web && npm run build`
