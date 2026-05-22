# Task: Extract shared lib helpers (epoch + creator + datetime-local)

Parent: `1500_story_web_spa_refinement.md`
Resolves: F-012, F-018, F-028, F-032 (review report
  `web_spa_2026-05-22.md`)

## Background

Four duplicated helpers, all small, all under `web/src/`:

### F-012 + F-028 — three near-identical epoch formatters

The `bigint | number` epoch coercion
(`typeof epoch === "bigint" ? Number(epoch) : epoch`) appears in:

- `web/src/pages/wrapper-permissions.tsx:72-86`
  (`formatRelativeFromEpoch`)
- `web/src/pages/wrapper-permissions.tsx:89-94`
  (`formatEpochFull`)
- `web/src/pages/client-instances.tsx:31-36` (`formatEpoch`)
- `web/src/pages/users.tsx:128-134` (`formatEpoch`)

Each renders either a relative or absolute label.
`formatRelativeFromEpoch` is exported and imported by
`wrapper-permissions.test.tsx:10`.

### F-018 — `describeCreator` defined twice

- `web/src/pages/secrets.tsx:532-542`
- `web/src/pages/wrapper-permissions.tsx:101-111`

Identical helper turning an `EntityCreator` into a short label
using a `displayNameById` lookup.

### F-032 — `toDatetimeLocal` for a single caller

- `web/src/pages/wrapper-permissions.tsx:1131-1139`

A standalone helper at the bottom of a 1139-line file, used once
(line 940). Belongs next to the other date helpers.

## Scope

- Create `web/src/lib/epoch.ts` exporting:
  - `normalizeEpoch(value: bigint | number | null | undefined): number | null` —
    returns `null` for null/undefined/0/NaN, otherwise the
    `number` value (with the bigint coercion folded in).
  - `formatEpochRelative(value, now?: number = Date.now()): string` —
    `"never"` for `null`, otherwise the `Intl.RelativeTimeFormat`
    rendering already in `wrapper-permissions.tsx`.
  - `formatEpochAbsolute(value): string` — `"never"` for `null`,
    otherwise `new Date(n * 1000).toLocaleString()`. Same shape
    as the current `formatEpochFull`.
  - `formatEpochDate(value): string` — date-only flavour
    (`toLocaleDateString()`), matching `users.tsx` /
    `client-instances.tsx`.
  - `toDatetimeLocal(date: Date): string` — moved verbatim from
    `wrapper-permissions.tsx:1131-1139`.

- Create `web/src/lib/creator.ts` exporting:
  - `describeCreator(creator: EntityCreator, displayNameById?: Map<string, string>): string` —
    moved verbatim.

- Replace the inline copies in all four pages. Update imports.
  Tests that imported `formatRelativeFromEpoch` from the page
  (`wrapper-permissions.test.tsx:10`) re-point to
  `@/lib/epoch`. The page keeps its `formatRelativeFromEpoch`
  export only if other call sites need it under the old name —
  prefer re-pointing tests instead so the page surface shrinks.

- Decide locally whether to keep `formatRelativeFromEpoch` as an
  alias or rename to `formatEpochRelative` everywhere. Recommended:
  rename for symmetry, one find-and-replace step in tests.

## Acceptance criteria

- [ ] `rg -n 'typeof epoch === "bigint"' web/src/pages/` returns
      nothing.
- [ ] `rg -n 'function describeCreator' web/src/pages/` returns
      nothing.
- [ ] `rg -n 'function toDatetimeLocal' web/src/pages/` returns
      nothing.
- [ ] `web/src/lib/epoch.ts` and `web/src/lib/creator.ts` exist
      and are imported by the four pages.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
