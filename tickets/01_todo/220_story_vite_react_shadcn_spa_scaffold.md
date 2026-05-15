# Story: New SPA scaffold (Vite + React + shadcn/ui)

Parent: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Background

The Next.js app in `apps/admin-ui/` is replaced by a Vite + React 19 + TypeScript SPA in a new top-level `web/` directory. shadcn/ui (Radix + Tailwind v4) provides component primitives. Routing via TanStack Router, server state via TanStack Query, forms via react-hook-form + zod. Type sharing with the Rust backend via ts-rs.

`apps/admin-ui/` is not deleted in this story — that happens in the cutover story (260) once the SPA reaches feature parity. During development the two coexist.

## Tasks

1. `220_task_web_app_scaffold_vite_react_tailwind_shadcn.md`
2. `221_task_tanstack_router_query_and_auth_guard.md`
3. `222_task_ts_rs_rust_to_ts_type_generation.md`
4. `223_task_login_logout_pages_forms.md`
5. `224_task_vitest_playwright_setup_and_baseline_e2e.md`

## Acceptance criteria

- [ ] `web/` directory exists with a working Vite + React + TS + Tailwind v4 + shadcn/ui app.
- [ ] Auth-guarded route tree redirects to `/login` on 401 (preserving intended destination).
- [ ] `web/src/api/types.ts` is generated from Rust DTOs and CI fails on drift.
- [ ] Login, logout, bootstrap pages work against the new Rust auth endpoints.
- [ ] Vitest unit tests and Playwright e2e tests run locally and in Nix.
- [ ] Mobile viewport smoke tests pass at 375×812.
