# Task: Vite + React + Tailwind v4 + shadcn/ui scaffold

Parent: `220_story_vite_react_shadcn_spa_scaffold.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Create `web/` with `package.json`, `vite.config.ts`, `tsconfig.json`, `tailwind.config.ts`, `postcss.config.js`.
- React 19 + TypeScript strict mode.
- Tailwind v4 with the project's existing dark-shell tokens carried over from `apps/admin-ui/`.
- shadcn/ui base components installed (copied into `web/src/components/ui/`): `button`, `input`, `label`, `form`, `dialog`, `sheet`, `dropdown-menu`, `tabs`, `table`, `badge`, `tooltip`, `toast`, `card`, `select`, `checkbox`, `switch`.
- Lucide icons.
- Base app shell with side navigation (drawer below `md`), header, dark theme by default with a light-mode toggle.
- Folder layout: `web/src/{api,components,features,hooks,lib,pages,routes,styles}`.

## Acceptance criteria

- [ ] `npm install && npm run build` produces `web/dist/`.
- [ ] `npm run dev` starts a local dev server that talks to a locally-running backendd via Vite proxy.
- [ ] Tailwind purges correctly; `dist/` size is reasonable (< 500 KB gzipped for the initial chunk).
- [ ] Sidenav becomes a drawer (`Sheet`) below `md` breakpoint.
- [ ] Vitest tests for the shell layout (drawer open/close, theme toggle) pass.
- [ ] New files are git-tracked (Nix flake snapshot gotcha).
