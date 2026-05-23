# Task: Adopt self-invalidating dialogs everywhere; drop parent-driven `onSuccess` callbacks

## Background

The SPA uses two cache-invalidation patterns in admin dialogs:

- **Self-invalidating** (majority): the dialog opens its own
  `useQueryClient`, runs the mutation, and calls
  `qc.invalidateQueries({ queryKey: ... })` from its `onSuccess`
  before closing.
- **Parent-driven** (minority): the page passes `onSuccess` /
  `onCreated` / `onDeleted` callbacks down; the dialog invokes
  them; the page is what calls `qc.invalidateQueries`.

Self-invalidating dialogs:
`web/src/pages/users/dialogs/create.tsx:58, 70-72`,
`web/src/pages/users/dialogs/delete.tsx:29, 33-36`,
`web/src/pages/stores/dialogs/create.tsx:49, 69-71`,
`web/src/pages/secrets/dialogs/write.tsx:62, 108-111`,
plus every other store / secret / user dialog.

Parent-driven dialogs:
`web/src/pages/client-instances/dialogs/{create,edit,delete,set-status}.tsx`
(wired from `client-instances/page.tsx:179-222`),
`web/src/pages/wrapper-permissions/dialogs/create-pair.tsx`
(wired from `wrapper-permissions/page.tsx:225-237`),
`web/src/pages/wrapper-permissions/dialogs/delete-pair.tsx`
(wired from `wrapper-permissions/page.tsx:240-254`).

Source review:
`tickets/_review/web_admin_spa_2026-05-23.md` F-013.

The self-invalidating pattern wins on two counts: the dialog
knows which keys its mutation touched (it does not need the page
to encode that knowledge), and removing the callback prop drops
~6 lines of plumbing per call site. Pick that one and apply it
everywhere.

## Scope

Convert each parent-driven dialog to self-invalidating. For every
dialog listed below:

1. Replace the `{ onSuccess: () => Promise<void> }` prop (called
   variously `onSuccess`, `onCreated`, `onDeleted`) with a single
   `{ onClose: () => void }` prop.
2. Inside the dialog, open `const qc = useQueryClient();`.
3. In the mutation's `onSuccess`, call `qc.invalidateQueries(...)`
   for the keys the page used to invalidate, then `onClose()`.

### Dialogs to convert

- `web/src/pages/client-instances/dialogs/create.tsx` —
  page invalidates `["admin", "client-instances"]`.
- `web/src/pages/client-instances/dialogs/edit.tsx` — same.
- `web/src/pages/client-instances/dialogs/set-status.tsx` — same.
- `web/src/pages/client-instances/dialogs/delete.tsx` — same.
- `web/src/pages/wrapper-permissions/dialogs/create-pair.tsx` —
  page invalidates
  `["admin", "wrapper-permissions-view", storeName]` (via
  `invalidatePairsAndPermissions` / `invalidateAggregateView`
  after ticket 2060 lands).
- `web/src/pages/wrapper-permissions/dialogs/delete-pair.tsx` —
  same key.

### Page-side cleanup

- `web/src/pages/client-instances/page.tsx:32-43, 179-222` — drop
  the `useQueryClient` import if it's only used by the dialog
  callbacks; remove the `onSuccess` props on dialog mounts.
- `web/src/pages/wrapper-permissions/page.tsx:101-106, 225-254` —
  the `invalidateAggregateView` helper has no callers left after
  the conversion; delete it. The page just mounts/unmounts
  dialogs by their `<Dialog>` open state.

### Verify the inverse

After the change, every admin dialog that mutates state owns its
own invalidation. `grep -n "onSuccess: () => Promise<void>"
web/src/pages/` should return zero hits.

## Out of scope

- The two dialogs that already self-invalidate but use the
  `onClose` prop for closing the parent's local UI state — that
  pattern stays.
- The non-mutating viewers (PolicyViewerDialog, RevealSecretDialog).

## Acceptance criteria

- [ ] `grep -rn "onSuccess: () => Promise<void>\|onCreated:\|onDeleted:"
      web/src/pages/` returns zero hits.
- [ ] Every dialog in the affected files opens its own
      `useQueryClient` and calls `qc.invalidateQueries` from the
      mutation's `onSuccess`.
- [ ] `invalidatePairsAndPermissions` / `invalidateAggregateView`
      no longer exists in
      `web/src/pages/wrapper-permissions/page.tsx`.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes (the
      `client-instances.spec.ts` and `wrappers.spec.ts` flows are
      the regression guard).

## Notes for the implementer

- This lands after ticket 2060, which renames
  `invalidatePairsAndPermissions` to `invalidateAggregateView`.
  By the time 2090 runs, that helper is dead code — delete it
  in this ticket.
- The client-instances dialogs are also being rewritten in 2030
  to use the Form pattern. Coordinate the order: do 2030 first
  (Form scaffolding lands), then 2090 (invalidation pattern
  changes). The diff for 2090 then only touches the mutation
  closures, not the JSX.
