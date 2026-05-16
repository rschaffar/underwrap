# Story: Admin SPA post-rework polish

No parent epic — standalone story collecting follow-ups discovered after
the Next.js → embedded-Vite cutover (commits `11852a6`, `79306e7`,
`37fd135`).

## Background

The SPA rework landed, but two patch commits on top of the main cutover
(`79306e7 "Drop nav entries that point at non-existent SPA routes"` and
`37fd135 "Restore web/dist/.gitkeep so rust-embed has a non-empty default"`)
revealed two classes of fragility:

1. **Nav-route drift.** Nav entries can point at routes that don't
   exist, and the only feedback loop is an operator hitting a 404.
2. **Embed correctness is untested.** `rust-embed` reads from
   `$UNDERWRAP_WEB_DIST` at compile time; if that directory is empty
   the binary builds and starts fine but serves nothing at `/`. No
   test catches this.

In addition, several SPA pages added during the rework do not yet have
Playwright e2e coverage (Users CRUD, Profile, Dashboard). The unit-test
side covers them via mocks, but the cutover-class breakages above are
exactly the kind of thing only an end-to-end test catches.

## Tasks

1. `291_task_enforce_nav_route_consistency.md`
2. `292_task_embedded_spa_smoke_test.md`
3. `293_task_users_page_playwright_e2e.md`
4. `294_task_profile_and_dashboard_e2e.md`

## Acceptance criteria

- [ ] It is structurally impossible to ship a nav entry that points at
      an unregistered route (enforced in code, not by code review).
- [ ] A test fails the build if the embedded SPA does not serve its
      index document.
- [ ] Users CRUD, Profile, and Dashboard each have at least one
      Playwright e2e covering the golden path.

## Out of scope

- New features. This story is strictly hardening the surface the
  rework already shipped.
- Secrets CRUD UI and Policy viewer UI — those are tracked separately
  in tickets 300 and 301 because they are scope decisions, not polish.
