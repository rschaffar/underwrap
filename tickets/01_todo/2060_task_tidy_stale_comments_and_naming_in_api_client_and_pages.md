# Task: Tidy stale comments, names, and one cache key

## Background

Four small drift fixes:

1. **`invalidatePairsAndPermissions` misnomer** — the helper at
   `web/src/pages/wrapper-permissions/page.tsx:101-106` only
   invalidates the single aggregate-view key. The apology comment
   at `:233-235` acknowledges this.
2. **Stale "(legacy …)" comment** at
   `web/src/pages/secrets/model.ts:33-37`: "The backend already
   guarantees the shape (legacy path-encoded ids never reach the
   wire), so there is no "unrecognized" bucket here." No such
   surface exists; greenfield rule says drop the parenthetical.
3. **Unenforced contract preamble** in
   `web/src/api/client.ts:104-150` — the
   "Anything that's never referenced has been deleted — re-add only
   when a real callsite surfaces" sentence is no longer guarded.
4. **`PolicyViewerDialog` cache key** uses `"policy"` but the
   fetcher hits `/v1/admin/stores/{id}/wrapper-permissions`
   (`web/src/components/policy-viewer-dialog.tsx:41-43`). Every
   other permissions query uses
   `["admin", "wrapper-permissions(-view)", storeName]`.

Source review:
`tickets/_review/web_admin_spa_2026-05-23.md` F-011, F-012, F-014, F-015.

## Scope

### Rename `invalidatePairsAndPermissions`

Rename to `invalidateAggregateView` in
`web/src/pages/wrapper-permissions/page.tsx:101-106`. Update the
two call sites at `:232` and `:250`. Delete the apology comment at
`:233-235` ("The aggregate view fetches resources too, so its
invalidation is enough.").

### Drop the stale parenthetical

In `web/src/pages/secrets/model.ts:33-37`, change:

> The backend already guarantees the shape (legacy path-encoded
> ids never reach the wire), so there is no "unrecognized" bucket
> here.

to:

> The backend already guarantees the shape, so there is no
> "unrecognized" bucket here.

### Shorten the schema-types preamble

Replace the 7-line "Named schema types" preamble in
`web/src/api/client.ts:104-111` with:

```ts
// ----- Named schema types -----
//
// Aliases for the OpenAPI schemas this client references by hand.
// Generated entries that aren't named here still live in
// `schema.ts` and can be reached via `components["schemas"]`.
type S = components["schemas"];
```

Drop the "Anything that's never referenced has been deleted — re-add
only when a real callsite surfaces" sentence and the
`export`/non-export rationale lines. Leave the alias list itself
untouched.

### Re-key the policy viewer

In `web/src/components/policy-viewer-dialog.tsx:41-50`, change the
`queryKey` from `["admin", "policy", storeName]` to
`["admin", "wrapper-permissions", storeName]`. Leave `staleTime:
0` and `refetchOnMount: "always"` in place (they're the right
choice for an operator who reopens the viewer to confirm an edit).

## Out of scope

- Reshaping `fetchWrapperPermissions` or the underlying endpoint.
- Renaming `clientInstancesForStore`, `sortPairs`, or
  `groupPairsByResource`. (`derivations.ts` stays.)
- Touching the alias list in `api/client.ts` (the actual aliases,
  not the preamble).

## Acceptance criteria

- [ ] `grep -rn "invalidatePairsAndPermissions" web/src/` returns
      zero hits.
- [ ] `grep -n "legacy path-encoded" web/src/pages/secrets/model.ts`
      returns no hits.
- [ ] `grep -n "Anything that's never referenced" web/src/api/client.ts`
      returns no hits.
- [ ] `grep -n '"policy"' web/src/components/policy-viewer-dialog.tsx`
      returns no hits.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes.
