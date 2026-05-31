# Task: Simplify stores/client-instances dialogs (casts, dead props, reset, comments)

## Problem

Findings F-01, F-02, F-03, F-04, F-06, F-07, F-09, F-15 in
`tickets/_review/stores-client-instances-ui_2026-05-31.md`:

- `stores/dialogs/create.tsx:18-22` — a `type` alias is interleaved inside
  the import block.
- `stores/dialogs/create.tsx:37` + `stores/page.tsx:69-73` — `CreateStoreDialog`
  is the only dialog with a dead `open` prop (always `true` when mounted).
- `stores/dialogs/edit.tsx:39`, `stores/dialogs/unlock.tsx:44` — redundant
  `as EditValues` / `as UnlockValues` casts on already-typed values.
- `stores/dialogs/create.tsx:61-63`, `stores/dialogs/rotate-key.tsx:40` —
  unsafe `as` casts on `unknown` hook results.
- `stores/dialogs/unlock.tsx:46-48,54-56,106-107` — three no-op
  `form.reset({ key_hex: "" })` calls (dialog is freshly mounted each open).
- `stores/dialogs/delete.tsx:33`, `stores/dialogs/rotate-key.tsx:35` —
  "via the override below" comments that describe a non-existent override.
- `client-instances/dialogs/create.tsx:58` — opaque
  `auto_create_wrapper_assets ? true : undefined`.
- `stores/dialogs/key-reveal.tsx:36` — dead SSR `typeof navigator` guard.

## What to do

1. Move the interleaved `type` alias below the imports (or inline it).
2. Drop the `open` prop from `CreateStoreDialog`; hardcode `open` on its
   `AdminDialog`; match the call site to every other dialog.
3. Remove the value casts (edit/unlock) and the result casts/guards
   (create/rotate-key). **Depends on shared-infra ticket 1430** (the
   `TResult` generic) for the result-typing half.
4. Delete the three `form.reset` calls in `unlock.tsx`.
5. Replace the delete/rotate "override" comments with an accurate note (or
   remove them).
6. Pass the `auto_create_wrapper_assets` boolean directly.
7. Reduce the key-reveal copy guard to `if (navigator.clipboard) { … }`.
8. Final category-7a re-read of each touched file.

## Acceptance

- `cd web && npm test`
- `cd web && npm run build`
