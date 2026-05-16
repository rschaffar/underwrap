# Task: Enforce nav-entry ↔ route consistency in code

Parent: `290_story_admin_spa_post_rework_polish.md`

## Background

Commit `79306e7` removed nav entries pointing at non-existent SPA
routes (`/wrappers`, `/audit`). The drift was only noticed when an
operator clicked a dead link. There is no code-level check that nav
entries correspond to registered routes.

## Scope

- In `web/src/components/app-shell.tsx` (or wherever the NAV array
  lives), derive nav entries from the registered route tree instead
  of maintaining a parallel hand-written list — or, if that's awkward
  given the TanStack Router setup, add a build-time / test-time
  assertion that every NAV entry's `to` field resolves to a real
  route.
- The check must fail loudly: a CI test or a TypeScript-level
  guarantee, not a runtime `console.warn`.
- A nav entry that is intentionally a placeholder (e.g. a route
  registered but feature-flagged off) must still be representable —
  the check enforces "exists in router tree", not "user can reach".

## Acceptance criteria

- [ ] Adding a nav entry whose `to` is not in the router tree fails
      `npm run build` or `npm test` — pick whichever fits the
      existing pipeline.
- [ ] The existing nav set passes the check.
- [ ] One short comment near the NAV array explains how to add a new
      entry safely (a single sentence, not a treatise).

## Out of scope

- Reorganizing the nav into groups / collapsing sections.
- Per-role nav filtering — that's a separate concern.
