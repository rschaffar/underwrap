# Task: Mobile responsive pass across all pages

Parent: `250_story_access_matrix_tab_and_mobile_polish.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Audit every SPA page at 375×812 and fix:
  - Sidenav → drawer (shadcn `Sheet`) below `md`
  - Header collapses logo/title appropriately
  - Tables that don't fit collapse to a card list (per-row `Card`)
  - Modals: full-screen below `sm`, large below `md`
  - All clickable controls have ≥44 px touch targets
  - No horizontal scrolling at 320 px width except inside intentional overflow containers (e.g. JSON view of a secret)
- Focus management on drawer open/close.
- Smoke-test dark and light theme at mobile widths.

## Acceptance criteria

- [ ] All pages render correctly at 375×812 and 768×1024.
- [ ] Drawer nav works with keyboard and screen reader.
- [ ] Visual regression baseline (Playwright screenshots) captured per page at three viewports.
- [ ] Vitest tests cover the table → card breakpoint switching logic.
