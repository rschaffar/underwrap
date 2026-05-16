# Story: Access matrix as a top-level tab + mobile polish pass

Parent: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Background

The access matrix is currently nested inside per-store wrapper pages. Operators consistently want to view and manage it as its own first-class view. The mobile experience also needs a deliberate pass since the new design has a sidenav-as-drawer pattern and operators will sometimes need to use the console from a phone (oncall scenarios).

## Tasks

1. `250_task_access_matrix_top_level_route.md`
2. `251_task_mobile_responsive_pass.md`
3. `252_task_mobile_and_matrix_e2e_tests.md`

## Acceptance criteria

- [ ] `/matrix` route is a top-level tab in the SPA shell nav.
- [ ] Matrix view supports store and wrapper filters with deep-link query params.
- [ ] Every page renders usably at 375×812 (iPhone SE / 12 Mini) and 768×1024 (iPad).
- [ ] Sidenav becomes a drawer below `md`; modals full-screen below `sm`; tables degrade to card lists where appropriate; tap targets ≥44px.
- [ ] Playwright runs at both phone and tablet viewports.
