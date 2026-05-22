# Task: Extract `useStoreSelector` and the resource/client-instance lookup hooks

Parent: `1500_story_web_spa_refinement.md`
Resolves: F-017, F-019, F-027 (review report
  `web_spa_2026-05-22.md`)

## Background

### F-017 — store-selector with URL persistence duplicated

`web/src/pages/secrets.tsx:125-145` and
`web/src/pages/wrapper-permissions.tsx:187-204` carry
character-identical logic for:

- Reading `?store=…` from `window.location.search` on first mount.
- Defaulting to the first available store when the URL parameter
  is absent.
- Writing the chosen store back to the URL via
  `window.history.replaceState`.

### F-019 — resource/client-instance lookup Maps duplicated

Both pages call the same two list queries
(`fetchClientInstances`, `postResourcesList(storeName)`) and
build the same two `Map<string, string>` lookups:

- `web/src/pages/secrets.tsx:167-180, 191-197` — `displayNameById`
  and `resourceNameById`.
- `web/src/pages/wrapper-permissions.tsx:228-241` — same two
  Maps, same shape.

### F-027 — IIFE for initial store

Both pages use
`const initialStore = (() => { ... })();` to read the URL once.
A lazy `useState` initializer does the same thing without the
IIFE.

## Scope

- Create `web/src/hooks/use-store-selector.ts`:

  ```ts
  export function useStoreSelector(
    availableStores: ReadonlyArray<{ id: string }> | undefined,
  ): { storeName: string | null; setStoreName: (id: string) => void };
  ```

  Implementation:
  - Lazy `useState` initializer reads `?store=` once (replaces the
    IIFE in both pages).
  - `useEffect` defaults to `availableStores[0]?.id` when
    `storeName` is null and a store becomes available.
  - `useEffect` writes back to the URL via
    `history.replaceState({}, "", url.toString())` on change.

- Create `web/src/hooks/use-store-lookups.ts` (or two narrower
  hooks if cleaner):

  ```ts
  export function useStoreLookups(storeName: string | null): {
    displayNameById: Map<string, string>;
    resourceNameById: Map<string, string>;
    resources: AdminResourcesListResponse["resources"];
    isLoading: boolean;
  };
  ```

  Owns the `useQuery(["admin", "client-instances"])` and
  `useQuery(["admin", "resources", storeName])` calls and
  returns the memoised Maps + the raw resource list for callers
  that need the records themselves.

- Replace the duplicated logic in
  `web/src/pages/secrets.tsx:125-197` and
  `web/src/pages/wrapper-permissions.tsx:187-241` with calls to
  the two hooks.

- Existing page tests (`secrets.test.tsx`,
  `wrapper-permissions.test.tsx`) keep passing: they mock the
  API surface, and the hooks consume the same `useQuery` calls.

## Acceptance criteria

- [ ] `web/src/hooks/use-store-selector.ts` and
      `web/src/hooks/use-store-lookups.ts` exist.
- [ ] `rg -n 'window.history.replaceState' web/src/pages/` returns
      nothing (the only sites today are in the two pages this
      ticket touches).
- [ ] `rg -n 'const initialStore' web/src/pages/` returns nothing.
- [ ] The duplicated `displayNameById` / `resourceNameById`
      `useMemo` blocks are gone from both pages.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
