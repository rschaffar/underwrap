# Task: Web — page-level dedup, naming, tiny cleanups

Resolves: W-PG-1, W-PG-2, W-PG-3, W-PG-4, W-PG-5, W-PG-6, W-PG-7,
W-PG-8, W-PG-9, W-PG-10.

## What to do

1. **`useResourceCatalog` hook** (W-PG-2):
   `web/src/pages/wrapper-permissions/page.tsx:52-59` and
   `web/src/pages/secrets/page.tsx:50-67` both compute
   `resourceNameById = new Map<string, string>` from
   `view.data.resources`. Lift into a hook (in `web/src/hooks/`)
   returning `{ nameById, recordById }`. Use from both pages.

2. **`permissionKey` helper** (W-PG-3):
   `web/src/pages/wrapper-permissions/page.tsx:77-82` and
   `web/src/pages/wrapper-permissions/table.tsx:111` both compose
   `${client_instance}|${wrapper}|${resource_id}` by hand. Add
   `permissionKey(clientInstance, wrapper, resourceId)` next to
   `wrapperResourceKey` in
   `web/src/pages/wrapper-permissions/derivations.ts`. Use on both
   sides.

3. **Inline or rename `secrets/model.ts`** (W-PG-4):
   `web/src/pages/secrets/model.ts` is the only `model.ts` in the
   codebase; the convention recurs nowhere. Either inline
   `groupSecretEntries` into `page.tsx`'s `useMemo`, or rename to
   `grouping.ts` to match
   `wrapper-permissions/derivations.ts`. Recommend rename for
   symmetry. (Note: `groupSecretEntries` may also shrink after
   `1040_task_narrow_secret_list_slot_schema` — coordinate.)

4. **Inline `ModeKind`** (W-PG-5):
   `web/src/pages/wrapper-permissions/derivations.ts` exports
   `ModeKind = WrapperApprovalMode["kind"]`. That's a type alias,
   not a derivation. Inline at its caller
   (`web/src/pages/wrapper-permissions/dialogs/edit-cell.tsx`).

5. **Flatten libsecret helpers** (W-PG-6):
   `web/src/pages/secrets/libsecret/index.ts` (one-line) and
   `internal.ts` (imported directly by every dialog) — the
   "internal" boundary isn't enforced by TypeScript. Pick:
   - **Recommended:** delete both files; move helpers to
     `web/src/pages/secrets/libsecret/helpers.ts` (the dialogs
     already import from `../internal` so the import paths just
     change).
   - Alternative: keep the structure but rename `internal.ts`
     to `helpers.ts`.

6. **`libsecretIdField` zod helper** (W-PG-7):
   `web/src/pages/secrets/libsecret/internal.ts:9-28` carries
   `ID_PATTERN` + `parseAttributes`. Three dialogs
   (`create-collection.tsx`, `create-item.tsx`, `set-alias.tsx`)
   repeat the `id`/`label` schema portion inline. Factor a
   `libsecretIdField()` zod helper emitting validator + canonical
   message.

7. **`--color-warning` token** (W-PG-8):
   `web/src/pages/secrets/libsecret/aliases-panel.tsx:71-72` uses
   `bg-(--color-warning)/20 text-(--color-warning)`. Every other
   warning UI uses concrete `amber-500/30` / `amber-200`. Verify
   the token is defined in `tailwind.config.*` / CSS. If not,
   replace with the amber palette like the other badges.

8. **`epochToDate` helper** (W-PG-9):
   `web/src/pages/wrapper-permissions/table.tsx:184-193` and
   `dialogs/edit-cell.tsx:52-55` both do `new Date(epoch * 1000)`.
   Add `epochToDate(epoch: number): Date` to `web/src/lib/epoch.ts`
   and use it on both sides.

9. **Move policy-viewer badge to page header** (W-PG-1):
   `web/src/components/policy-viewer-dialog.tsx:82-83,103-113`
   shows a "writable permissions: N of M" badge above JSON the
   operator is about to read. Move it to the
   wrapper-permissions page header (or drop entirely). The
   policy viewer becomes just the JSON.

10. **Decide `EntityCreator` label format** (W-PG-10):
    `web/src/lib/creator.ts:8-18` returns `"client {name}"` as a
    bare-prefix string while other places render just the name with
    a badge. Pick:
    - Return structured `{ label: string, kind: "user" |
      "client-instance" | ... }` and let the renderer decorate
      consistently (preferred — matches the wrapper-permissions
      table badges).
    - Or drop the prefix and let each caller add context.

## Acceptance

- `cd web && npm test`
- `cd web && npm run build`
- Visual check: every warning badge looks the same; epoch
  formatting matches between table and edit-cell dialog.
