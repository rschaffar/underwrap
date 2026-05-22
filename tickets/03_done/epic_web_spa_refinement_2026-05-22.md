# Epic: First-pass refinement of the `web/` admin SPA (2026-05-22)

## Background

First focused review of the `web/` admin SPA, captured in
`tickets/_review/web_spa_2026-05-22.md`. The component is in good
shape — clean shadcn/Tailwind/TanStack foundation, careful no-log
API client exercised by unit tests, broad RTL + Playwright coverage.
The drag on the first-release look is structural:

- **Four page modules over 700 LoC** (three over 1000) because every
  page hosts all of its dialogs inline. `secrets.tsx` 1385,
  `wrapper-permissions.tsx` 1139, `stores.tsx` 1095,
  `client-instances.tsx` 715, plus `users.tsx` at 629 marginally over.
- **`api/client.ts` carries 140 LoC of import-then-reexport** of 75
  generated types — pure boilerplate the ts-rs generator already
  maintains.
- **Patterns are copy-pasted across pages**: a store-selector with
  URL persistence (2× verbatim), `describeCreator` (2×), three
  near-identical epoch formatters, 25+ inline `role="alert"` divs,
  6+ inline `<select>` styled the same way, four pages declaring
  a `principal?` prop they never read.
- **`WrapperApprovalMode` cast soup**: 11 `as`-cast sites because
  TypeScript widens object literals and ts-rs renders `u64` as
  `bigint`. Fixable with a small Rust annotation tweak plus
  SPA-side cleanup.
- **Two ticket references in source/docs** (project-wide sweep
  rule).

## Goals

1. **Extract the shared building blocks first** (lib helpers, hooks,
   alert + select primitives). 1510–1540 land before any page
   split so the page splits inherit them.
2. **Collapse `api/client.ts`'s type-reexport block** to a single
   `export type * from "@/api/types"`. ~140 LoC removed.
3. **Drop the unused `principal` prop** on four pages and the
   matching route-wrapper components in `router.tsx`.
4. **Clean up `WrapperApprovalMode` typing.** Annotate
   `until_epoch` as `number` on the Rust side so ts-rs emits
   `number` instead of `bigint`; then drop every SPA-side
   `as WrapperApprovalMode` and `(mode as { until_epoch })` cast,
   using `as const` or explicit variable annotations to keep TS
   narrowing happy.
5. **Split the five oversize page modules** into
   `pages/<name>/{page,table,…}.tsx` + per-dialog files. Every
   resulting `page.tsx` lands under 300 LoC.
6. **Tail cleanup**: drop unused deps (`sonner`,
   `@radix-ui/react-toast`, possibly `@tanstack/router-plugin`),
   the empty `afterEach` in one test, the local `within`
   reimplementation, the stale `key file` assertion, the
   "carried over from apps/admin-ui" comment.
7. **Repo-wide sweep**: strip the two surviving `story NNN`
   pointers in `web/src/api/client.ts` and `docs/backend.md`.

## Stories

- `1500_story_web_spa_refinement.md` — single rollout story
  enumerating the task tickets and their landing order.

## Product decisions

- **No behaviour change.** Every task is a pure refactor. The
  one cross-crate touch (1570 — `WrapperApprovalMode`) flips a
  TypeScript-only annotation; the JSON wire shape is identical.
- **One PR per task.** Each task ticket leaves the tree green
  (`cd web && npm test && npm run build`; for 1570 also
  `cargo test --workspace`, `cargo fmt -- --check`, and
  `cargo clippy --all-targets --workspace -- -D warnings`).
- **Order matters for two anchors:** 1570 lands before 1600
  (wrapper-permissions split), so the split inherits the
  clean type; 1550 lands before any page split, so the
  splits don't have to maintain the import/re-export block.
  The other ticket ordering is suggested-not-required.
- **Playwright e2e suite gates the page splits.** Each split
  ticket must keep `cd web && npm run test:e2e` green; the
  helper extractions (1510–1560) only need the vitest suite.

## Acceptance criteria

- [ ] All 13 task tickets are in `tickets/03_done/`.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` (which runs `tsc --noEmit`) passes.
- [ ] `cd web && npm run test:e2e` passes (Playwright; run inside
      `nix develop` after `cargo build -p backend-backendd && cd web
      && npm run build`).
- [ ] `cargo test --workspace` passes (gated by 1570).
- [ ] `cargo fmt -- --check` and
      `cargo clippy --all-targets --workspace -- -D warnings` pass.
- [ ] `rg -n -i '(ticket[[:space:]]*[0-9]{3,4}|epic_[a-z_]+|story_[a-z_]+|story \d+|stories \d+|TODO\([0-9]{3,4}\)|FIXME\([0-9]{3,4}\))' web/src/ docs/ examples/ crates/ nix/`
      returns nothing.
- [ ] `rg -n 'as WrapperApprovalMode|as unknown as WrapperApprovalMode' web/src/`
      returns nothing.
- [ ] No `pages/*.tsx` file exceeds 600 LoC (the five splits
      restructure under `pages/<name>/`).
- [ ] No `principal?:` prop declaration survives in `stores.tsx`,
      `wrapper-permissions.tsx`, `secrets.tsx`, or
      `client-instances.tsx` (or their new `page.tsx`
      equivalents). `users.tsx` / `profile.tsx` keep theirs.

## Out of scope

- Wider "render u64 as number" decision for every timestamp field.
  Touched narrowly inside 1570 for `until_epoch` only; broader
  flip is a separate ticket if/when needed.
- Toast system. The unused `sonner` and `@radix-ui/react-toast`
  deps are dropped; if a toast UI is wanted later, that's a fresh
  feature ticket.
- `@tanstack/router-plugin` re-wiring. 1630 drops it if unused or
  moves it to `devDependencies`; wiring it for router code-gen
  is out of scope.
- Migration of the inline-everything dialog style to a `useDialog`
  hook + shared open/close registry. Could land later if dialog
  proliferation continues.
- Security review of the admin auth/secret-reveal flows. No
  security-boundary findings surfaced in this pass.

## Review report

`tickets/_review/web_spa_2026-05-22.md` — full findings with
`file:line` citations and per-ticket finding-ID mappings.
