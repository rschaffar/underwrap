# Task: Add client-instance delete/re-enable tests; harden store button selectors

## Problem

Findings F-12, F-13 in
`tickets/_review/stores-client-instances-ui_2026-05-31.md`:

- `client-instances.test.tsx` has no case exercising
  `DeleteClientInstanceDialog` / `deleteClientInstance`, nor the Re-enable
  path on a disabled instance.
- `stores.test.tsx:89-92,125` selects action buttons by position
  (`buttons[0]`/`buttons[1]`), which breaks silently if a new action
  button is added to a row.

## What to do

1. Add a client-instance delete test (asserts `deleteClientInstance` with
   the right id) and a re-enable test (asserts `putClientInstanceUpdate`
   with `{ status: "active" }`).
2. Replace positional button indexing in `stores.test.tsx` with
   role+name selectors
   (`within(row).getByRole("button", { name: /^edit$/i })`, etc.),
   matching the technique already used elsewhere in the file.

## Acceptance

- `cd web && npm test` (with the new/updated tests)
- `cd web && npm run build`
