# Task: Rename SPA `storeName` to `storeId` end-to-end

Resolves: S-1.

## Problem

The SPA threads `storeName` through every per-store page, dialog, and
hook, but the value is always the backend's `id` field — never the
operator-visible display name. The variable name actively lies; readers
of the code spend cognitive effort reconciling it.

`useStoreSelector(availableStores)` defaults to `availableStores[0].id`;
the URL key is `?store=<id>`. The backend's path parameter is `id`.

## Files touched

Roughly 30 files. The starting points:

- `web/src/hooks/use-store-selector.ts:18-30`
- `web/src/components/policy-viewer-dialog.tsx:21-23`
- `web/src/pages/wrapper-permissions/page.tsx:39,46`
- `web/src/pages/secrets/dialogs/*`
- `web/src/pages/secrets/libsecret/*`

…and every other site that imports the `storeName` prop/state.

## What to do

1. Rename the `storeName` variable, prop, and hook return field to
   `storeId` everywhere it appears in `web/src/`.
2. Update the URL search-param key consistently. If keeping `?store=`
   in the URL (it's operator-typed), keep the param name; just rename
   the variable.
3. Do not change any wire shape — the backend's path parameter is
   already `id`.

## Acceptance

- `cd web && npm run build` (passes `tsc --noEmit`)
- `cd web && npm test`
- Manual walk-through: `/stores`, `/secrets`, `/wrapper-permissions`
  with multiple stores selected; URL `?store=` and the selector stay
  in sync.
- No reference to `storeName` left in non-generated `web/src/` files
  (verify with `rg storeName web/src --type ts --type tsx`).
