import { createContext, StrictMode, useContext, useState } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import {
  createBrowserHistory,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Link,
  Outlet,
  RouterProvider,
  type RouterHistory,
} from "@tanstack/react-router";
import {
  QueryClient,
  QueryClientProvider,
  useQuery,
} from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useStoreSelector } from "./use-store-selector";

const STORES = [{ id: "primary" }, { id: "secondary" }] as const;
type Stores = ReadonlyArray<{ id: string }> | undefined;
const StoreList = createContext<Stores>(undefined);
const dispose: Array<() => void> = [];

// Match the authenticated layout's permissive search validator.
function validateSearch(
  search: Record<string, unknown>,
): Record<string, unknown> & { store?: string } {
  const first = Array.isArray(search.store) ? search.store[0] : search.store;
  const result = { ...search };
  delete result.store;
  return {
    ...result,
    ...(typeof first === "string" ||
    typeof first === "number" ||
    typeof first === "boolean"
      ? { store: String(first) }
      : {}),
  };
}

async function renderSelector(
  initialStores: Stores,
  initialPath = "/secrets?store=primary",
  browser = false,
) {
  if (browser) {
    // Seed before constructing the real browser router; never mutate native
    // history underneath a mounted router or synthesize popstate events.
    window.history.replaceState(null, "", initialPath);
  }
  const history: RouterHistory = browser
    ? createBrowserHistory()
    : createMemoryHistory({ initialEntries: [initialPath] });
  if (browser) {
    // The history adapter initializes its own state keys. Seed user state
    // through that adapter before mounting, rather than before its creation.
    history.replace(initialPath, {
      ...history.location.state,
      marker: "user-state",
      nested: { count: 1 },
    });
    history.flush?.();
  }
  const replace = vi.spyOn(history, "replace");
  const details = vi.fn(async (_id: string) => null);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  dispose.push(() => {
    history.destroy();
    queryClient.clear();
  });
  let updateStores!: (stores: Stores) => void;

  function Harness() {
    const { storeId, setStoreId } = useStoreSelector(useContext(StoreList));
    useQuery({
      queryKey: ["detail", storeId],
      queryFn: () => details(storeId!),
      enabled: storeId !== null,
    });
    return (
      <>
        <output data-testid="effective-store">{storeId ?? "none"}</output>
        {["primary", "secondary", "missing"].map((id) => (
          <button key={id} onClick={() => setStoreId(id)}>
            Select {id}
          </button>
        ))}
        <Link to="/secrets" search={storeId === null ? {} : { store: storeId }}>
          Secrets
        </Link>
        <Link
          to="/wrappers"
          search={storeId === null ? {} : { store: storeId }}
        >
          Wrappers
        </Link>
      </>
    );
  }
  const root = createRootRoute({ component: Outlet });
  const layout = createRoute({
    getParentRoute: () => root,
    id: "app",
    validateSearch,
    component: Outlet,
  });
  const secrets = createRoute({
    getParentRoute: () => layout,
    path: "/secrets",
    component: Harness,
  });
  const wrappers = createRoute({
    getParentRoute: () => layout,
    path: "/wrappers",
    component: Harness,
  });
  const router = createRouter({
    history,
    routeTree: root.addChildren([layout.addChildren([secrets, wrappers])]),
    defaultPendingMinMs: 0,
  });
  function Provider() {
    const [stores, setStores] = useState(initialStores);
    updateStores = setStores;
    return (
      <StoreList.Provider value={stores}>
        <QueryClientProvider client={queryClient}>
          <RouterProvider router={router} />
        </QueryClientProvider>
      </StoreList.Provider>
    );
  }
  render(
    <StrictMode>
      <Provider />
    </StrictMode>,
  );
  await screen.findByTestId("effective-store");
  await waitFor(() => expect(router.state.status).toBe("idle"));
  return {
    history,
    router,
    replace,
    details,
    updateStores: (stores: Stores) => updateStores(stores),
    setStores: async (stores: Stores) => {
      await act(async () => updateStores(stores));
      await waitFor(() => expect(router.state.status).toBe("idle"));
    },
  };
}

function effectiveStore() {
  return screen.getByTestId("effective-store").textContent;
}

function expectContext(
  history: RouterHistory,
  store: string | null,
  hash = "#resource",
) {
  const url = new URL(history.location.href, "https://example.test");
  expect(url.searchParams.getAll("store")).toEqual(
    store === null ? [] : [store],
  );
  expect(url.searchParams.get("keep")).toBe("yes");
  expect(url.hash).toBe(hash);
}

afterEach(() => {
  cleanup();
  for (const release of dispose.splice(0)) release();
  vi.restoreAllMocks();
  window.history.replaceState(null, "", "/");
});

describe("useStoreSelector with the real router", () => {
  it("gates details while unresolved and canonicalizes one stale fallback in StrictMode", async () => {
    const view = await renderSelector(
      undefined,
      "/secrets?store=stale&keep=yes#resource",
    );
    expect(effectiveStore()).toBe("none");
    expect(view.replace).not.toHaveBeenCalled();
    expect(view.details).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("Select primary"));
    expect(view.replace).not.toHaveBeenCalled();
    await view.setStores(STORES);
    await waitFor(() => expect(effectiveStore()).toBe("primary"));
    expectContext(view.history, "primary");
    expect(view.replace).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(view.details).toHaveBeenCalledWith("primary"));
    expect(view.details.mock.calls.every(([id]) => id === "primary")).toBe(
      true,
    );
  });

  it("falls back after removal and removes only store for a resolved-empty list", async () => {
    const view = await renderSelector(
      STORES,
      "/secrets?store=secondary&keep=yes#resource",
    );
    expect(effectiveStore()).toBe("secondary");
    expect(view.replace).not.toHaveBeenCalled();
    view.details.mockClear();
    await view.setStores([STORES[0]]);
    expect(effectiveStore()).toBe("primary");
    expectContext(view.history, "primary");
    expect(view.details.mock.calls.every(([id]) => id === "primary")).toBe(
      true,
    );
    view.details.mockClear();
    await view.setStores([]);
    expect(effectiveStore()).toBe("none");
    expectContext(view.history, null);
    expect(view.replace).toHaveBeenCalledTimes(2);
    expect(view.details).not.toHaveBeenCalled();
  });

  it("keeps a canonical URL untouched, rejects invalid setters, and replaces a valid choice", async () => {
    const view = await renderSelector(
      STORES,
      "/secrets?store=secondary&keep=yes#resource",
      true,
    );
    const length = window.history.length;
    const userState = {
      marker: window.history.state.marker,
      nested: window.history.state.nested,
    };
    fireEvent.click(screen.getByText("Select missing"));
    fireEvent.click(screen.getByText("Select secondary"));
    expect(effectiveStore()).toBe("secondary");
    expect(view.replace).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(screen.getByText("Select primary"));
    });
    await waitFor(() => expect(effectiveStore()).toBe("primary"));
    expectContext(view.history, "primary");
    expect(window.history.length).toBe(length);
    expect(window.history.state).toMatchObject(userState);
    expect(view.replace).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByText("Select primary"));
    expect(view.replace).toHaveBeenCalledTimes(1);
  });

  it("preserves user history state, query and hash when defaulting", async () => {
    const initialLength = window.history.length;
    const view = await renderSelector(
      STORES,
      "/secrets?keep=yes#resource",
      true,
    );
    expect(effectiveStore()).toBe("primary");
    expectContext(view.history, "primary");
    expect(window.history.state).toMatchObject({
      marker: "user-state",
      nested: { count: 1 },
    });
    expect(window.history.length).toBe(initialLength);
    expect(view.replace).toHaveBeenCalledTimes(1);
  });

  it("canonicalizes duplicate keys to the first valid requested ID", async () => {
    const view = await renderSelector(
      STORES,
      "/secrets?store=primary&store=secondary&keep=yes#resource",
    );
    expect(effectiveStore()).toBe("primary");
    expectContext(view.history, "primary");
    expect(view.replace).toHaveBeenCalledTimes(1);
    await act(async () => {});
    expect(view.replace).toHaveBeenCalledTimes(1);
    await act(async () => {
      await view.router.navigate({
        href: "/secrets?store=primary&store=secondary&keep=yes#resource",
      });
    });
    await waitFor(() => expectContext(view.history, "primary"));
  });

  it("revalidates stale IDs on real router navigation without ever fetching that ID", async () => {
    const view = await renderSelector(STORES);
    view.details.mockClear();
    await act(async () => {
      await view.router.navigate({
        href: "/secrets?store=stale&keep=yes#resource",
      });
    });
    await waitFor(() => expectContext(view.history, "primary"));
    expect(effectiveStore()).toBe("primary");
    expect(view.details.mock.calls.every(([id]) => id === "primary")).toBe(
      true,
    );
  });

  it("retains selection across sections and follows actual browser Back and Forward", async () => {
    await renderSelector(STORES, "/secrets?store=secondary", true);
    await act(async () => {
      fireEvent.click(screen.getByRole("link", { name: "Wrappers" }));
    });
    await waitFor(() => expect(window.location.pathname).toBe("/wrappers"));
    expect(effectiveStore()).toBe("secondary");
    await act(async () => {
      fireEvent.click(screen.getByText("Select primary"));
    });
    await waitFor(() => expect(effectiveStore()).toBe("primary"));
    await act(async () => {
      window.history.back();
    });
    await waitFor(() => {
      expect(window.location.pathname).toBe("/secrets");
      expect(effectiveStore()).toBe("secondary");
    });
    await act(async () => {
      window.history.forward();
    });
    await waitFor(() => {
      expect(window.location.pathname).toBe("/wrappers");
      expect(effectiveStore()).toBe("primary");
    });
  });

  it("revalidates an obsolete selection reached through actual browser Back", async () => {
    const view = await renderSelector(
      STORES,
      "/secrets?store=secondary&keep=yes#resource",
      true,
    );
    await act(async () => {
      await view.router.navigate({ href: "/wrappers?store=primary" });
    });
    await view.setStores([STORES[0]]);
    view.details.mockClear();
    await act(async () => {
      window.history.back();
    });
    await waitFor(() => {
      expect(window.location.pathname).toBe("/secrets");
      expect(effectiveStore()).toBe("primary");
      expectContext(view.history, "primary");
    });
    expect(view.details.mock.calls.every(([id]) => id === "primary")).toBe(
      true,
    );
  });

  it("does not let a departing route canonicalize a new route with its old list", async () => {
    const view = await renderSelector(undefined, "/secrets?store=stale");
    // Make the old route eligible for canonicalization while navigating to a
    // distinct valid destination; no native history or hook internals are mocked.
    await act(async () => {
      const navigation = view.router.navigate({
        href: "/wrappers?store=secondary&keep=yes#policy",
      });
      view.updateStores(STORES);
      await navigation;
    });
    await waitFor(() => expect(effectiveStore()).toBe("secondary"));
    expect(view.history.location.pathname).toBe("/wrappers");
    expectContext(view.history, "secondary", "#policy");
    expect(view.details.mock.calls.every(([id]) => id === "secondary")).toBe(
      true,
    );
  });
});
