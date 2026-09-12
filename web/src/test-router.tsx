import type { ReactNode } from "react";
import {
  Outlet,
  RouterProvider,
  createBrowserHistory,
  createRootRoute,
  createRoute,
  createRouter,
  type RouterHistory,
} from "@tanstack/react-router";
import { afterEach } from "vitest";

const histories: RouterHistory[] = [];
afterEach(() => {
  for (const history of histories.splice(0)) history.destroy();
});

/** Real router ownership for page units, not an authentication harness. */
export function withTestRouter(
  children: ReactNode,
  pages: Readonly<Record<string, ReactNode>> = {},
  historyState?: Record<string, unknown>,
) {
  // Construct once outside React so StrictMode cannot create/dispose duplicate
  // native-history adapters. The test owns cleanup after the actual unmount.
  const root = createRootRoute({ component: Outlet });
  const routes = [
    "/",
    "/secrets",
    "/wrappers",
    "/stores",
    "/client-instances",
    "/setup",
    "/users",
    "/profile",
  ].map((path) =>
    createRoute({
      getParentRoute: () => root,
      path,
      validateSearch: (search) => search,
      component: () => pages[path] ?? children,
    }),
  );
  const history = createBrowserHistory();
  if (historyState) {
    history.replace(history.location.href, historyState);
    history.flush?.();
  }
  histories.push(history);
  const router = createRouter({
    history,
    routeTree: root.addChildren(routes),
    defaultPendingMinMs: 0,
  });
  return <RouterProvider router={router} />;
}
