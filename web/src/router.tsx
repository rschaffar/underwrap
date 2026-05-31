import {
  Outlet,
  RouterProvider,
  createRootRoute,
  createRoute,
  createRouter,
  redirect,
} from "@tanstack/react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { AppShell } from "@/components/app-shell";
import { LoginPage } from "@/pages/login";
import { BootstrapPage } from "@/pages/bootstrap";
import { WrapperPermissionsPage } from "@/pages/wrapper-permissions/page";
import { SecretsPage } from "@/pages/secrets/page";
import { StoresPage } from "@/pages/stores/page";
import { ClientInstancesPage } from "@/pages/client-instances/page";
import { UsersPage } from "@/pages/users/page";
import { ProfilePage } from "@/pages/profile";
import {
  ApiError,
  type WhoamiResponse,
  fetchBootstrapStatus,
  fetchWhoami,
  postLogout,
} from "@/api/client";
import { Button } from "@/components/ui/button";

interface RouterContext {
  queryClient: QueryClient;
  principal: WhoamiResponse | null;
}

// Listing queries are invalidated on mutation `onSuccess`, so a
// non-zero `staleTime` would only delay refetches without adding
// value. Per-query overrides should be rare (use a longer
// `staleTime` for genuinely background-cacheable data like
// `whoami` if it ever becomes a bottleneck).
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      retry: 1,
      staleTime: 0,
    },
  },
});

const rootRoute = createRootRoute({
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

const appLayoutRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: "app",
  beforeLoad: async ({ location }) => {
    try {
      const principal = await fetchWhoami();
      if (principal.username != null) {
        return { principal };
      }
    } catch (err) {
      // Network or schema error — fall through to the bootstrap probe
      // so the user lands somewhere actionable instead of a blank page.
      if (!(err instanceof ApiError)) {
        throw err;
      }
    }
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
  component: AppLayoutComponent,
});

function AppLayoutComponent() {
  return (
    <AppShell>
      <Outlet />
    </AppShell>
  );
}

const indexRoute = createRoute({
  getParentRoute: () => appLayoutRoute,
  path: "/",
  beforeLoad: () => {
    throw redirect({ to: "/secrets", replace: true });
  },
});

const storesRoute = createRoute({
  getParentRoute: () => appLayoutRoute,
  path: "/stores",
  component: StoresPage,
});

const clientInstancesRoute = createRoute({
  getParentRoute: () => appLayoutRoute,
  path: "/client-instances",
  component: ClientInstancesPage,
});

const usersRoute = createRoute({
  getParentRoute: () => appLayoutRoute,
  path: "/users",
  component: function UsersRouteComponent() {
    const ctx = usersRoute.useRouteContext();
    return <UsersPage principal={ctx.principal} />;
  },
});

const profileRoute = createRoute({
  getParentRoute: () => appLayoutRoute,
  path: "/profile",
  component: function ProfileRouteComponent() {
    const ctx = profileRoute.useRouteContext();
    return <ProfilePage principal={ctx.principal} />;
  },
});

const wrapperPermissionsRoute = createRoute({
  getParentRoute: () => appLayoutRoute,
  path: "/wrappers",
  component: WrapperPermissionsPage,
});

const secretsRoute = createRoute({
  getParentRoute: () => appLayoutRoute,
  path: "/secrets",
  component: SecretsPage,
});

// ----- Public auth routes -----

interface LoginSearch {
  next?: string;
}

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/login",
  validateSearch: (search): LoginSearch => {
    const safe = safeNextPath(
      typeof search.next === "string" ? search.next : null,
    );
    return safe ? { next: safe } : {};
  },
  component: function LoginRouteComponent() {
    const { next } = loginRoute.useSearch();
    const navigate = loginRoute.useNavigate();
    return (
      <LoginPage
        onSuccess={async () => {
          await queryClient.invalidateQueries();
          await navigate({ to: next ?? "/", replace: true });
        }}
      />
    );
  },
});

const bootstrapRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/bootstrap",
  component: function BootstrapRouteComponent() {
    const navigate = bootstrapRoute.useNavigate();
    return (
      <BootstrapPage
        onSuccess={async () => {
          await queryClient.invalidateQueries();
          await navigate({ to: "/" });
        }}
      />
    );
  },
});

const logoutRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/logout",
  beforeLoad: async () => {
    try {
      await postLogout();
    } catch {
      // Even if the server rejects (already revoked, CSRF, etc.) we still
      // proceed to /login — the session cookie is either gone server-side
      // or never existed.
    }
    queryClient.clear();
    throw redirect({ to: "/login", replace: true });
  },
  component: function LogoutRouteComponent() {
    return (
      <div className="flex min-h-screen items-center justify-center p-6">
        <p className="text-sm text-(--color-muted)">Signing out…</p>
      </div>
    );
  },
});

/**
 * Resolve a `?next=` query parameter to a same-origin path, or `null` if
 * it is missing/malformed/cross-origin. Critical: we must NOT redirect
 * to an attacker-controlled URL after sign-in (open redirect).
 */
export function safeNextPath(next: string | null): string | null {
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
  appLayoutRoute.addChildren([
    indexRoute,
    wrapperPermissionsRoute,
    secretsRoute,
    storesRoute,
    clientInstancesRoute,
    usersRoute,
    profileRoute,
  ]),
  loginRoute,
  bootstrapRoute,
  logoutRoute,
]);

const router = createRouter({
  routeTree,
  context: { queryClient, principal: null } as RouterContext,
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
