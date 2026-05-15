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
  component: () => {
    const navigate = useNavigate();
    return (
      <AppShell
        onLogout={async () => {
          try {
            await postLogout();
          } catch {
            // Even if the server rejects (already revoked), proceed
            // to the login screen — the cookies have been cleared
            // server-side or never existed.
          }
          await navigate({ to: "/login" });
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

// ----- Public auth routes -----

const loginRoute = new Route({
  getParentRoute: () => rootRoute,
  path: "/login",
  component: LoginPagePlaceholder,
});

const bootstrapRoute = new Route({
  getParentRoute: () => rootRoute,
  path: "/bootstrap",
  component: BootstrapPagePlaceholder,
});

function LoginPagePlaceholder() {
  const navigate = useNavigate();
  // Until ticket 223 lands the real form, this placeholder just lets
  // the user retry the whoami round-trip after they've signed in
  // elsewhere (e.g. a curl flow). The guard will redirect on success.
  useEffect(() => {
    document.title = "underwrap — login";
  }, []);
  return (
    <div className="flex min-h-screen items-center justify-center p-6">
      <div className="w-full max-w-sm rounded-xl border border-(--color-line) bg-(--color-surface-0)/70 p-6">
        <h2 className="text-lg font-semibold text-(--color-fg)">Sign in</h2>
        <p className="mt-2 text-sm text-(--color-muted)">
          The full login form lands in ticket 223. Once you have a
          session cookie, click below to continue.
        </p>
        <Button
          className="mt-4 w-full"
          onClick={() => {
            void navigate({ to: "/" });
          }}
        >
          Continue
        </Button>
      </div>
    </div>
  );
}

function BootstrapPagePlaceholder() {
  useEffect(() => {
    document.title = "underwrap — first-time setup";
  }, []);
  return (
    <div className="flex min-h-screen items-center justify-center p-6">
      <div className="w-full max-w-sm rounded-xl border border-(--color-line) bg-(--color-surface-0)/70 p-6">
        <h2 className="text-lg font-semibold text-(--color-fg)">First-time setup</h2>
        <p className="mt-2 text-sm text-(--color-muted)">
          No operators exist yet. The full bootstrap form lands in
          ticket 223; for now, use the CLI or the JSON endpoint to
          create the first admin.
        </p>
      </div>
    </div>
  );
}

const routeTree = rootRoute.addChildren([
  appLayoutRoute.addChildren([indexRoute]),
  loginRoute,
  bootstrapRoute,
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
