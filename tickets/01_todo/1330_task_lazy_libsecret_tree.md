# Task: Lazy `LibsecretTree` + dedup policy-viewer query

Resolves: S-7, S-8.

## Problem

### 1. `LibsecretTree` fires N queries on page mount (S-8)

`web/src/pages/secrets/libsecret/tree.tsx:31-34` mounts inside each
resource block (under a `<details>` summary) and unconditionally
`useQuery`s its libsecret data. For a store with N libsecret-enabled
resources, that's N round-trips on every page visit — regardless of
whether the operator ever expands any of them.

### 2. Duplicate policy-viewer query (S-7)

The wrapper-permissions page already fetches the aggregate
`/v1/admin/stores/{id}/wrapper-permissions-view` (returns
`permissions + pairs + resources`):

`web/src/pages/wrapper-permissions/page.tsx:44-48`

The PolicyViewerDialog opens *from inside that page* and fetches the
smaller `/v1/admin/stores/{id}/wrapper-permissions` endpoint:

`web/src/components/policy-viewer-dialog.tsx:42-50`

Two cache entries for the same store, two round-trips when the dialog
opens.

## What to do

### For LibsecretTree

Two options; pick (b) if the backend can support it cleanly.

**(a)** Gate the query on `<details>` open state:
- Track `open: boolean` via `useState` + `onToggle` on the `<details>`.
- Pass `enabled: open` to the `useQuery` call.

**(b)** Fold libsecret into the existing `/secrets-view` aggregate
so the secrets page makes one round-trip total. The aggregate
returns `{ resources, secrets: {env, files, libsecret: {...} per
resource} }`. Backend already aggregates pairs/resources/permissions
for the same store on the wrapper-permissions endpoint — same pattern.

Recommendation: (b). Adds a backend change but removes the round-trip
problem entirely and keeps the SPA simpler.

### For PolicyViewerDialog

Pass the `permissions` array down from the wrapper-permissions page
as a prop. The dialog is opened from inside that page; the cache is
already warm.

Delete the dialog's `useQuery` for `/v1/admin/stores/{id}/wrapper-permissions`
and the corresponding query key.

(Alternative: if the dialog ever needs to be opened from a context
where `permissions` isn't already loaded, share the cache by reading
from the aggregate `wrapper-permissions-view` query key with
`queryClient.getQueryData(...)`. Recommendation: pass as prop —
explicit data flow beats cache lookups.)

## Acceptance

- `cd web && npm test`
- `cd web && npm run build`
- Manual check with a store carrying ≥5 libsecret-enabled resources:
  - DevTools network tab on `/secrets` shows **one** view round-trip
    (option b) or **zero** libsecret round-trips until a `<details>`
    is expanded (option a).
  - Opening the policy viewer from `/wrapper-permissions` does **not**
    fire an additional XHR.
