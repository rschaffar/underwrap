# Task: Mobile and matrix end-to-end tests

Parent: `250_story_access_matrix_tab_and_mobile_polish.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Configure Playwright `projects` for `desktop-1280`, `tablet-768`, `mobile-375`.
- Mobile-specific e2e:
  - Open drawer nav, navigate to Matrix, close drawer
  - Reveal secret modal renders full-screen, copy works
  - Resource list shows card layout
  - Create-store dialog usable on mobile (admin)
- Matrix e2e (across all viewports):
  - Filters persist across reload via URL params
  - Toggle a cell; reload; toggle persists
  - Optimistic rollback on simulated 500 from `/v1/admin/ui/matrix/...`
  - Operator without grant to any store sees an empty-state on the matrix page

## Acceptance criteria

- [ ] Playwright runs all three viewport projects.
- [ ] Mobile drawer + matrix toggle covered.
- [ ] Empty-state for ungranted operator covered.
- [ ] No flake-prone waits — use Playwright's auto-waiting and explicit `expect` polling where needed.
