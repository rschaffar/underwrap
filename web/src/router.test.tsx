import { describe, expect, it } from "vitest";

import { NAV } from "@/components/app-shell";
import { APP_ROUTE_PATHS, appRoutes } from "@/router";

describe("router ↔ nav consistency", () => {
  it("APP_ROUTE_PATHS matches the set of registered app routes", () => {
    const declared = new Set<string>(APP_ROUTE_PATHS);
    const registered = new Set<string>(
      appRoutes.map((route) => {
        // TanStack Router's `RouteOptions` is a union (path-routes vs
        // id-routes). Every entry in `appRoutes` is a path-route by
        // construction, so we narrow with a runtime guard rather than
        // a non-portable type assertion.
        const opts = route.options as { path?: string };
        if (typeof opts.path !== "string") {
          throw new Error(
            `appRoutes contains a non-path route (id=${(opts as { id?: string }).id ?? "?"})`,
          );
        }
        return opts.path;
      }),
    );
    expect(registered).toEqual(declared);
  });

  it("every NAV entry points at a path in APP_ROUTE_PATHS", () => {
    // The `href` field on `NavItem` is typed as `AppRoutePath` so
    // this is also enforced at compile time. The runtime assertion
    // here documents the contract and catches the case where a
    // future refactor weakens the type.
    const declared = new Set<string>(APP_ROUTE_PATHS);
    for (const item of NAV) {
      expect(declared).toContain(item.href);
    }
  });
});
