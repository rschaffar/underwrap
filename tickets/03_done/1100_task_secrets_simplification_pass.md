# Task: Simplify secrets dialogs (dead branches, redundant casts/wrappers, memo, state typing)

## Problem

Same-behaviour cruft across the `secrets` page subtree (findings F-01,
F-02, F-03, F-04, F-08 in
`tickets/_review/secrets-libsecret-ui_2026-05-31.md`):

- `dialogs/write.tsx:110-115` (and the `FormDescription` ternary at
  120-122) carries an unreachable third slot arm — `SecretSlot` is exactly
  `"env" | "files"` (`grouping.ts:5` → `schema.ts:1136`).
- Six `submit` callbacks cast their already-typed argument:
  `dialogs/write.tsx:57`, `dialogs/create-resource.tsx:40`,
  `dialogs/rename-resource.tsx:54`,
  `libsecret/dialogs/create-collection.tsx:43`,
  `libsecret/dialogs/create-item.tsx:59`,
  `libsecret/dialogs/set-alias.tsx:48`.
- `libsecret/aliases-panel.tsx:21-27` lists a freshly-built `entries`
  array in a `useMemo` dep, defeating the memo.
- Four delete dialogs wrap `confirm` in a needless lambda
  (`dialogs/delete-secret.tsx:37`,
  `libsecret/dialogs/delete-alias.tsx:36`,
  `libsecret/dialogs/delete-collection.tsx:37`,
  `libsecret/dialogs/delete-item.tsx:38`).
- `page.tsx:81-83` restates `Omit<SecretAddress, "key">` inline.

## What to do

1. Collapse the `write.tsx` placeholder/description ternaries to a
   two-branch form and delete the unreachable arm.
2. Remove the six `as XValues` casts and use the callback argument
   directly. **Depends on shared-infra ticket 1430** (the `useFormDialog`
   `TResult`/typing fix); once landed, the argument is already typed.
3. Fix the `aliases-panel` memo to depend on `[aliases, collections]` and
   build `Object.entries(aliases)` inside the memo body.
4. Pass `onConfirm={confirm}` directly in the four delete dialogs.
5. Type the `page.tsx` `creating` state as `Omit<SecretAddress, "key"> | null`.
6. Final category-7a re-read of each touched file; remove anything that
   still fits the simplification lens.

## Acceptance

- `cd web && npm test`
- `cd web && npm run build`
