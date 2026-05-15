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

- [ ] Auth guard redirects 401 → /login → original destination after success.
- [ ] CSRF token attached automatically to all non-GET requests.
- [ ] Vitest tests cover the `api` wrapper (CSRF header, credentials, error mapping, no body logging).
- [ ] Manual smoke: visit `/`, get redirected to `/login`, log in, land back on `/`. (Playwright coverage in ticket 224.)
