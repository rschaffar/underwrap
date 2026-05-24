# Task: Router shell, auth guard, login `search` schema, and small cleanups

Resolves: W-API-5, W-API-6, W-API-7 (low-priority, optional),
W-RTR-1, W-RTR-2, W-RTR-3, W-RTR-4, W-RTR-5, W-RTR-6, W-RTR-7,
W-RTR-8, W-RTR-9.

## Problem

Several small router/shell awkwardnesses; bundled because they all
touch `router.tsx`, `app-shell.tsx`, and adjacent hooks.

## What to do

1. **Auth guard tree** (W-RTR-2):
   `web/src/router.tsx:47-84,128-145` — move the auth check from
   `rootRoute.beforeLoad` to `appLayoutRoute.beforeLoad`. Public
   routes (`/login`, `/bootstrap`) are siblings of `appLayoutRoute`
   that never traverse it. Drop the two pathname-equality checks
   in `rootRoute`.

2. **Logout side-effect in `beforeLoad`** (W-RTR-3):
   `web/src/router.tsx:256-282` — the `useEffect`-based
   `LogoutRouteComponent` double-fires under StrictMode.
   Move `postLogout()`, `queryClient.clear()`, and navigation into
   `logoutRoute.beforeLoad`. Component renders a static spinner.

3. **Required `AppShell.onLogout`** (W-RTR-4):
   `web/src/components/app-shell.tsx:70-74,120-131` — one caller
   always passes the prop; the "optional" branch (hide the logout
   button) is dead. Make required, or drop the prop and have
   AppShell navigate to `/logout` directly.

4. **Drop `APP_ROUTE_PATHS` + custom invariant** (W-RTR-5):
   `web/src/router.tsx:154-163,304-312`;
   `web/src/components/app-shell.tsx:29-47`. Use TanStack Router's
   typed `<Link to="/secrets">` instead of `<a href={path}>`; the
   framework type-checks the path natively. Delete
   `APP_ROUTE_PATHS` and the `routePathsAreConsistent` test.

5. **Login `validateSearch` for `?next=`** (W-RTR-6):
   `web/src/router.tsx:215-230,284-299` — define
   `loginRoute.validateSearch` that coerces `next` to a same-origin
   path string or null. Delete `safeNextPath` and the
   `window.location.search` access.

6. **Drop `defaultPreload: "intent"`** (W-RTR-7):
   `web/src/router.tsx:321-325`. All data is fetched via page-level
   `useQuery`, not route loaders — preload-on-hover has nothing to
   warm. Drop the option (or commit to loaders, which is a bigger
   rework left for later).

7. **Delete `App.tsx`** (W-RTR-1):
   `web/src/App.tsx` is `return <AppRouter />`. Have `main.tsx`
   render `<AppRouter />` directly; delete `App.tsx`.

8. **Global query stale-time strategy** (W-RTR-8):
   `web/src/router.tsx:37-45` sets `staleTime: 30_000` globally;
   `web/src/components/policy-viewer-dialog.tsx:48-50` overrides
   with `staleTime: 0, refetchOnMount: "always"`. Pick one
   convention:
   - **Recommended:** `staleTime: 0` for listing queries (already
     invalidated on mutation `onSuccess`); 30s only for
     `whoami`/bootstrap-status. Drop the per-query override.
   Document the convention in a short comment on the QueryClient
   creation.

9. **Dashboard** (W-RTR-9):
   `web/src/pages/dashboard.tsx` ships three cards with em-dashes
   for Stores / Wrappers / Operators. Pick:
   - Redirect `/` to `/secrets` (operator's actual landing page).
   - Or wire real counts (one extra query per metric, easy).
   Don't ship em-dashes.

10. **`useStoreSelector` URL writes** (W-API-5):
    `web/src/hooks/use-store-selector.ts:18-30` — drop the
    unconditional URL-write effect; have the `<Select onChange>`
    update both state and URL in one place. Single source of truth.

11. **Rename hook file** (W-API-6):
    `web/src/hooks/use-store-lookups.ts` →
    `web/src/hooks/use-client-instance-directory.ts`. The export's
    actual purpose is client-instance lookup, not store lookup.

12. **`useDocumentTitle` consolidation (optional)** (W-API-7):
    Low priority. If touching nearby files, consider folding into
    a `hooks/index.ts` barrel. Skip if not in scope of other
    changes here.

## Acceptance

- `cd web && npm test`
- `cd web && npm run test:e2e` (Playwright, inside `nix develop`)
- `cd web && npm run build`
- Manual: log in, log out, hit `/login?next=/secrets`, navigate
  the NAV bar — all transitions still work.
