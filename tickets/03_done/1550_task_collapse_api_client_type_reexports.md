# Task: Collapse `api/client.ts` type re-exports

Parent: `1500_story_web_spa_refinement.md`
Resolves: F-006 (partial — the type re-export half), F-007 (review
  report `web_spa_2026-05-22.md`)

## Background

`web/src/api/client.ts:125-265` carries two parallel 70-line blocks:

- Lines 125-194: `import type { ... } from "@/api/types"` listing
  75 type names used inside the file.
- Lines 196-265: `export type { ... }` re-exporting the same 75
  names so callers can write
  `import { type AdminStoreCreateRequest } from "@/api/client"`.

The body of `client.ts` consumes the imports internally (for the
per-endpoint helper signatures). The re-export block exists only
for callers that grew the habit of importing types from
`@/api/client` instead of `@/api/types`. The `ts_export` test
already maintains the barrel at `web/src/api/types/index.ts`, so
this block is pure boilerplate that has to be touched on every
DTO add/rename.

## Scope

- Replace lines 125-265 of `web/src/api/client.ts` with a single
  barrel line:

  ```ts
  export type * from "@/api/types";
  ```

  (TypeScript 5.0+; the project is on 5.9.x.)

- Below that, keep the type-only imports the file actually uses
  internally — but pull them from `@/api/types` directly so the
  file no longer has to maintain its own subset:

  ```ts
  import type {
    AdminStoreCreateRequest,
    // …only the names referenced inside this file
  } from "@/api/types";
  ```

  Prune the import list to just what the per-endpoint helpers
  reference; TypeScript's `noUnusedLocals` will flag any leftover.

- Existing callers that imported types from `@/api/client`
  continue to work because of the `export type *`. No call-site
  changes required.

- Verify: `cd web && npm run build` runs `tsc --noEmit`, which
  catches any caller that referenced a type the prune-down dropped
  from the internal import list. (None expected — every caller
  goes through the `export type *`.)

## Acceptance criteria

- [ ] `web/src/api/client.ts` line count drops by ~140.
- [ ] `rg -n '^export type \{' web/src/api/client.ts` returns
      nothing.
- [ ] The file contains exactly one
      `export type * from "@/api/types";` line.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
