# Task: Collapse `api/client.ts` wrapper boilerplate

Resolves: W-API-1, W-API-2, W-API-3, W-API-4.

## Problem

`web/src/api/client.ts` is 669 lines. About 500 of those (lines
156-669) are one-line-per-endpoint wrappers of the form:

```ts
export function postSomething(args) {
  return unwrap(client.POST("/path/{x}", { params: { path: { x } }, body }));
}
```

openapi-fetch already provides full type safety via the generated
`paths` from `schema.ts`; these wrappers add nothing beyond renaming
the call. A separate block at lines 109-154 carries ~12 type aliases
(`AdminClientInstanceUpdateRequest`, `AdminPairCreateRequest`, etc.)
that exist solely because individual wrappers consume them.

Additional issues:

- **Test-only export** (W-API-3):
  `web/src/api/client.ts:48-51` exports
  `_resetCsrfTokenForTesting()` to production. Only `_*ForTesting`
  export in the codebase; out of character.

- **`unwrap`'s dishonest signature** (W-API-4):
  `web/src/api/client.ts:89-102` has
  `return (data ?? (undefined as unknown)) as T;`
  The double-cast hides that 204 endpoints actually return nothing.

## What to do

1. Replace the per-endpoint wrappers with a thin general API surface.
   Either:
   - **Option A (preferred):** export `api.GET / api.POST / api.PUT /
     api.DELETE` that compose `unwrap(client.METHOD(...))` once.
     Call sites read `await api.POST("/v1/admin/stores/{id}/lock", {
     params: { path: { id } } })` — typed by openapi-fetch.
   - **Option B:** drop the wrapper layer entirely; have call sites
     import `client` + `unwrap` and call `unwrap(client.GET(...))`
     directly.
   Option A is recommended — call sites stay terse, and the single
   wrap point centralizes auth/CSRF concerns.
2. Migrate every call site (page components and dialogs) to the new
   shape.
3. Delete the ~12 local-only `type` aliases (W-API-2). Keep the
   named-type re-exports that call sites actually import (the
   "exported" ones with `export type`).
4. Replace `_resetCsrfTokenForTesting` with `vi.resetModules()` in
   `web/src/api/client.test.ts`'s setup, or guard the export behind
   `import.meta.env.MODE === "test"` (W-API-3).
5. Change `unwrap`'s signature to honest behaviour (W-API-4):
   ```ts
   async function unwrap<T>(...): Promise<T | undefined>
   ```
   …and let callers narrow. Or split into `unwrap` (returns `T`,
   for endpoints with a body) and `unwrapVoid` (returns `void`,
   for 204 endpoints). Drop the `(undefined as unknown) as T` cast.

## Acceptance

- `cd web && npm test`
- `cd web && npm run build` (TS type-check passes)
- `web/src/api/client.ts` shrinks substantially — target under 250
  lines.
- Grep: no remaining `_*ForTesting` exports in production files.
