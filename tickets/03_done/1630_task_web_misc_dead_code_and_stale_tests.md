# Task: Web SPA misc dead code and stale tests cleanup

Parent: `1500_story_web_spa_refinement.md`
Resolves: F-008, F-009, F-011, F-013, F-029, F-030, F-031 (review
  report `web_spa_2026-05-22.md`). Also touches the F-023
  decision branch if 1610 didn't already cover it.

## Background

Tail of independent small wins surfaced in the review. Each
sub-bullet is independent.

## Scope

### F-008 — unused deps in `web/package.json`

- `web/package.json:21` — `sonner` is not imported anywhere
  (`rg -n 'sonner' web/src/ web/e2e/` returns nothing).
- `web/package.json:31` — `@radix-ui/react-toast` likewise unused.
- Drop both from `dependencies`. Run `npm install` to update
  the lockfile.

### F-009 — `@tanstack/router-plugin` placement

- `web/package.json:24` — `@tanstack/router-plugin` is in
  `dependencies` (should be `devDependencies` if it's a build
  plugin) and is not referenced in `vite.config.ts` or anywhere
  under `web/src/`.
- Verify it is unused with `rg -n 'router-plugin' web/`. If
  unused, drop it. If the project plans to wire router
  code-gen later, move it to `devDependencies` and add a
  one-line code comment in `vite.config.ts` explaining the
  intent. Recommended: drop it; re-add later when actually
  wiring code-gen.

### F-011 — empty `afterEach` block

- `web/src/pages/wrapper-permissions.test.tsx:19` —
  `afterEach(() => {});` with no body. Delete the line.

### F-013 — local `within` reimplementation in `stores.test.tsx`

- `web/src/pages/stores.test.tsx:243-278` — a 35-line
  hand-rolled `within(scope)` that duplicates
  `@testing-library/react`'s `within`. Every other test file
  in `pages/` already imports it directly.
- Replace the local function with
  `import { within } from "@testing-library/react"` at the top
  of the file. Existing call sites work unchanged (the
  testing-library API is a superset).

### F-029 — stale `key file` assertion in `stores.test.tsx`

- `web/src/pages/stores.test.tsx:117-119` —
  `expect(screen.queryByLabelText(/key file/i)).toBeNull();`
  with the comment "The encrypted-only key_file field should
  NOT be visible while backend == plain (the default)."
- The create-store dialog no longer has a `key file` field
  (the backend allocates the key and shows it via
  `KeyRevealDialog` after creation). The assertion passes
  vacuously.
- Choose one:
  - **Delete the assertion** and the comment.
  - **Rewrite to assert what is shown when
    `backend === "encrypted"`**: the amber warning banner at
    lines 444-453 of the (pre-split) `stores.tsx`. Verify the
    banner is hidden for `plain` and visible for `encrypted`.
  - Recommended: rewrite — it's more valuable as a regression
    test of the actual current UI.

### F-030 — misleading rotate-disabled comment

- `web/src/pages/stores.test.tsx:166-171` — comment promises
  an assertion about the rotate button being disabled when the
  store is locked, but the test only asserts the lock button
  is absent on the locked row.
- Either drop the rotate sentence from the comment, or extend
  the test to open the edit dialog on the locked row and
  verify the rotate button is disabled (recommended — both
  document and verify the contract).

### F-031 — stale "carried over from apps/admin-ui" comment

- `web/src/styles/globals.css:3` — `Dark-shell tokens carried
  over from apps/admin-ui/app/globals.css.` `apps/admin-ui/`
  no longer exists.
- Replace with a one-line description such as `/* Dark-shell
  design tokens; Tailwind v4 reads them via @theme. */`.

### F-023 follow-up (only if not already covered by 1610)

- If ticket 1610 (`Split client-instances`) deferred narrowing
  `toDetail` away, narrow the dialog prop types to a
  hand-written `ClientInstanceSummary` here. Otherwise nothing
  to do.

## Acceptance criteria

- [ ] `rg -n '"sonner"|"@radix-ui/react-toast"' web/package.json`
      returns nothing.
- [ ] `rg -n '"@tanstack/router-plugin"' web/package.json`
      either returns nothing or shows the dep under
      `devDependencies` with a one-line intent comment in
      `vite.config.ts`.
- [ ] `web/package-lock.json` regenerated.
- [ ] `rg -n 'afterEach\(\(\) => \{\}\)' web/src/` returns
      nothing.
- [ ] `web/src/pages/stores.test.tsx` imports `within` from
      `@testing-library/react`; the local `function within`
      is gone.
- [ ] `web/src/pages/stores.test.tsx` no longer carries the
      vacuous `key file` assertion (or carries the rewritten
      version).
- [ ] `web/src/styles/globals.css` no longer mentions
      `apps/admin-ui`.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes.
