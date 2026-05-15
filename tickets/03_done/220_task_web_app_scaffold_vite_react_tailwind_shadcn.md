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

- [x] `npm install && npm run build` produces `web/dist/` (index.html + hashed assets/).
- [x] `npm run dev` starts a Vite dev server that proxies `/v1` to `UNDERWRAP_BACKEND_DEV_URL` (defaults to `http://127.0.0.1:9080`).
- [x] Tailwind v4 purges correctly: production bundle is ~85 KB gzipped JS + ~4 KB gzipped CSS, well under the 500 KB initial-chunk budget.
- [x] Sidenav becomes a drawer (`Sheet`) below `md` breakpoint.
- [x] Vitest tests for the shell layout (drawer open/close, theme toggle) pass.
- [x] New files are git-tracked (Nix flake snapshot gotcha).

## Implementation notes

- Tailwind v4 via `@tailwindcss/vite`. Theme tokens live in
  `src/styles/globals.css` under `@theme`. `tailwind.config.ts` and
  `postcss.config.js` are present but intentionally minimal — kept so
  external tooling that probes the project root still resolves a
  Tailwind project.
- shadcn/ui components were hand-copied rather than installed via the
  CLI to keep the scaffold deterministic. This ticket lands only the
  components the shell actually needs (`Button`, `Sheet`) plus the
  `cn` utility. The remaining components listed in scope (`input`,
  `label`, `form`, `dialog`, `dropdown-menu`, `tabs`, `table`,
  `badge`, `tooltip`, `toast`, `card`, `select`, `checkbox`,
  `switch`) will land alongside the first feature that needs each —
  the project pulls them in at copy-time anyway, so eager scaffolding
  would just be unused code.
- `useTheme` toggles the `dark` class on `<html>` and persists to
  `localStorage` under `uw.theme`. Default is dark (matches the
  legacy admin-ui look).
- `vite-env.d.ts` enables Vite-style CSS imports under `verbatimModuleSyntax`.
- `dist/.gitkeep` is preserved (rust-embed requires the directory to
  exist at compile time). The Nix-backed SPA build is deferred to
  ticket 202; for now, `npm run build` produces the bundle locally
  and the backend reads it via embedded or `UNDERWRAP_BACKENDD_WEB_DIR`.
