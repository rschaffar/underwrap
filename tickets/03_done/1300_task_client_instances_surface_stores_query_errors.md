# Task: Surface stores-query errors/loading on the client-instances page

## Problem

Finding F-05 (must-fix) in
`tickets/_review/stores-client-instances-ui_2026-05-31.md`.

`client-instances/page.tsx` renders the Create/Edit dialogs only when
`storesQuery.data` is present (`{creating && storesQuery.data ? … : null}`
at lines 176/183). The "Create client instance" button (line 88) has no
`disabled` guard — its `onClick` just sets `creating = true` (line 90). So
if the stores fetch errors or is still loading, clicking Create/Edit flips
the state but the dialog never mounts: a silent no-op with no error or
spinner. Verified: the button has no current `disabled` prop, so this is a
reachable path on backend failure.

## What to do

1. Surface `storesQuery.error` in a visible banner/alert on the page.
2. While `storesQuery` is pending or errored, set `disabled` on the Create
   button (line 88) and the per-row Edit buttons, with a tooltip
   explaining why, instead of letting the click no-op.

## Acceptance

- `cd web && npm test` — add a test asserting the error surfaces (and the
  Create button is disabled) when the stores query fails.
- `cd web && npm run build`
