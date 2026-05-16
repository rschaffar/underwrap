import {
  Outlet,
  RootRoute,
  Route,
  Router,
  RouterProvider,
  redirect,
  useNavigate,
} from "@tanstack/react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useEffect } from "react";

import { AppShell } from "@/components/app-shell";
import { Dashboard } from "@/pages/dashboard";
import { LoginPage } from "@/pages/login";
import { BootstrapPage } from "@/pages/bootstrap";
import { StoresPage } from "@/pages/stores";
import {
  ApiError,
  fetchBootstrapStatus,
  fetchWhoami,
  postLogout,
  type WhoamiResponse,
} from "@/api/client";
import { Button } from "@/components/ui/button";

interface RouterContext {
  queryClient: QueryClient;
  principal: WhoamiResponse | null;
}

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      retry: 1,
      staleTime: 30_000,
    },
  },
});

const rootRoute = new RootRoute({
  beforeLoad: async ({ location }) => {
    // Skip auth resolution on the public auth routes; the guard is
    // applied below on every protected child by re-checking principal.
    if (
      location.pathname === "/login" ||
      location.pathname === "/bootstrap"
    ) {
      return { principal: null };
    }
    try {
      const principal = await fetchWhoami();
      if (principal.authenticated) {
        return { principal };
      }
    } catch (err) {
      // Network or schema error — fall through to the bootstrap probe
      // so the user lands somewhere actionable instead of a blank page.
      if (!(err instanceof ApiError)) {
        throw err;
      }
    }
    // Unauthenticated. Decide between /login and /bootstrap.
    let bootstrapRequired = false;
    try {
      bootstrapRequired = (await fetchBootstrapStatus()).bootstrap_required;
    } catch {
      // Network error: prefer the login redirect over silently breaking.
    }
    throw redirect({
      to: bootstrapRequired ? "/bootstrap" : "/login",
      search: { next: location.href },
    });
  },
  component: RootLayout,
  notFoundComponent: NotFound,
  errorComponent: ErrorScreen,
});

function RootLayout() {
  return <Outlet />;
}

function ErrorScreen({ error }: { error: Error }) {
  return (
    <div className="flex min-h-screen items-center justify-center p-6">
      <div className="max-w-md rounded-xl border border-(--color-line) bg-(--color-surface-0)/70 p-6 text-center">
        <h2 className="text-lg font-semibold text-(--color-fg)">
          Something went wrong
        </h2>
        <p className="mt-2 text-sm text-(--color-muted)">
          {error instanceof ApiError ? error.message : "The page failed to load."}
        </p>
        <Button
          className="mt-4"
          onClick={() => {
            window.location.reload();
          }}
        >
          Reload
        </Button>
      </div>
    </div>
  );
}

function NotFound() {
  return (
    <div className="flex min-h-screen items-center justify-center p-6 text-center">
      <div>
        <h2 className="text-lg font-semibold">Not found</h2>
        <p className="mt-2 text-sm text-(--color-muted)">
          The page you requested doesn't exist.
        </p>
      </div>
    </div>
  );
}

// ----- Protected app routes -----

const appLayoutRoute = new Route({
  getParentRoute: () => rootRoute,
  id: "app",
  component: function AppLayoutComponent() {
    const navigate = useNavigate();
    const ctx = appLayoutRoute.useRouteContext();
    return (
      <AppShell
        role={ctx.principal?.role}
        onLogout={async () => {
          // Delegate the actual POST to the `/logout` route so there's
          // one place that knows how to revoke + clear caches + redirect.
          await navigate({ to: "/logout" });
        }}
      >
        <Outlet />
      </AppShell>
    );
  },
});

const indexRoute = new Route({
  getParentRoute: () => appLayoutRoute,
  path: "/",
  component: Dashboard,
});

const storesRoute = new Route({
  getParentRoute: () => appLayoutRoute,
  path: "/stores",
  component: function StoresRouteComponent() {
    // `principal` is set on the root route's beforeLoad; pass it
    // down so admin-gating doesn't re-fetch.
    const ctx = storesRoute.useRouteContext();
    return <StoresPage principal={ctx.principal} />;
  },
});

// ----- Public auth routes -----

function LoginRouteComponent() {
  const navigate = useNavigate();
  return (
    <LoginPage
      onSuccess={async () => {
        await queryClient.invalidateQueries();
        // Honour `?next=` if it points at a same-origin pathname; otherwise
        // fall back to the dashboard. Never follow arbitrary URLs from the
        // query string — that would be an open redirect.
        const search = new URLSearchParams(window.location.search);
        const safe = safeNextPath(search.get("next"));
        await navigate({ to: safe ?? "/", replace: true });
      }}
    />
  );
}

const loginRoute = new Route({
  getParentRoute: () => rootRoute,
  path: "/login",
  component: LoginRouteComponent,
});

function BootstrapRouteComponent() {
  const navigate = useNavigate();
  return (
    <BootstrapPage
      onSuccess={async () => {
        await queryClient.invalidateQueries();
        await navigate({ to: "/" });
      }}
    />
  );
}

const bootstrapRoute = new Route({
  getParentRoute: () => rootRoute,
  path: "/bootstrap",
  component: BootstrapRouteComponent,
});

function LogoutRouteComponent() {
  const navigate = useNavigate();
  useEffect(() => {
    void (async () => {
      try {
        await postLogout();
      } catch {
        // Even if the server rejects (already revoked, CSRF, etc.) we still
        // proceed to /login — the session cookie is either gone server-side
        // or never existed.
      }
      queryClient.clear();
      await navigate({ to: "/login", replace: true });
    })();
  }, [navigate]);
  return (
    <div className="flex min-h-screen items-center justify-center p-6">
      <p className="text-sm text-(--color-muted)">Signing out…</p>
    </div>
  );
}

const logoutRoute = new Route({
  getParentRoute: () => rootRoute,
  path: "/logout",
  component: LogoutRouteComponent,
});

/**
 * Resolve a `?next=` query parameter to a same-origin path, or `null` if
 * it is missing/malformed/cross-origin. Critical: we must NOT redirect
 * to an attacker-controlled URL after sign-in (open redirect).
 */
function safeNextPath(next: string | null): string | null {
  if (!next) return null;
  try {
    const url = new URL(next, window.location.origin);
    if (url.origin !== window.location.origin) return null;
    if (!url.pathname.startsWith("/")) return null;
    return url.pathname + url.search + url.hash;
  } catch {
    return null;
  }
}

const routeTree = rootRoute.addChildren([
  appLayoutRoute.addChildren([indexRoute, storesRoute]),
  loginRoute,
  bootstrapRoute,
  logoutRoute,
]);

const router = new Router({
  routeTree,
  context: { queryClient, principal: null } as RouterContext,
  defaultPreload: "intent",
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

export function AppRouter() {
  return (
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}
