# Task: TanStack Router + Query with auth-guarded routes

Parent: `220_story_vite_react_shadcn_spa_scaffold.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- TanStack Router (file-based routes under `web/src/routes/`).
- Root route runs a `beforeLoad` that calls `GET /v1/admin/auth/whoami`:
  - 401 → redirect to `/login?next=<intended>`
  - response indicates `bootstrap_required` → redirect to `/bootstrap`
  - 200 with principal → continue, principal available via router context
- TanStack Query client configured: `refetchOnWindowFocus: false`, `retry: 1`, default stale time 30s.
- Global error boundary renders a friendly error card with retry; global suspense boundary renders a skeleton.
- A typed `api` client wraps `fetch`:
  - includes credentials (`credentials: "include"`)
  - reads `__Host-uw-csrf` cookie and sets `X-CSRF-Token` on non-GET requests
  - throws typed errors `{code, message, status}` on non-2xx, mapped from the backend's stable error codes
  - never logs request bodies or response bodies (paranoia against accidental secret logging)

## Acceptance criteria

- [x] Auth guard redirects unauthenticated requests to `/login` (or `/bootstrap` when `bootstrap_required: true`) with the original location in `?next=`. Manual smoke + Playwright coverage in 224.
- [x] CSRF token attached automatically to all non-GET requests via the `apiFetch` wrapper.
- [x] Vitest tests cover the `api` wrapper: CSRF header on POST, no CSRF on GET, `credentials: "include"` always, typed `ApiError` with backend code, fallback code for non-JSON errors, and a regression test that no console output contains request or response body material.
- [x] Manual smoke: planned in ticket 224 (Playwright). The router placeholder pages are wired so the full login form in ticket 223 plugs into the existing redirect destination.

## Implementation notes

- Code-based TanStack Router (not file-based): keeps the route graph
  greppable in `src/router.tsx` and avoids bootstrapping the
  `@tanstack/router-plugin` Vite codegen for what is currently three
  routes. Switching to file-based is mechanical when the route count
  grows.
- `beforeLoad` on the root route fans out: try `whoami`; if
  unauthenticated, probe `bootstrap/status`; redirect to whichever
  page applies. Public auth routes (`/login`, `/bootstrap`)
  short-circuit the guard so they don't recurse.
- Login and bootstrap pages are placeholders for ticket 223 — they
  render enough UI to let a manually-acquired cookie continue into
  the shell.
- Vitest's jsdom URL is pinned to `https://underwrap.test/` so
  `__Host-` cookies (which require `Secure`) behave the same in
  tests as in production.
- The `api` client deliberately never `console.log`s bodies. A
  paranoia test asserts that no console method emits the secret
  material it just exchanged. This is the regression net for any
  future "add request logging" temptation.
