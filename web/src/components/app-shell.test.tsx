import { describe, expect, it, beforeEach, afterEach } from "vitest";
import {
  cleanup,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";

import type { AdminStoreListItem } from "@/api/client";
import { AppShell } from "./app-shell";

const PRODUCTION: AdminStoreListItem = {
  id: "production",
  name: "Production",
  backend: "encrypted",
  lifecycle_state: "locked",
  usage: { mapped_client_instance_count: 0, has_secrets: false },
  actions: {
    can_update: true,
    can_migrate: false,
    can_rotate_key: false,
    can_unlock: true,
    can_lock: false,
    can_delete_now: true,
    can_force_delete: false,
  },
};
const releases: Array<() => void> = [];

async function renderShell(
  props: Omit<ComponentProps<typeof AppShell>, "children"> = {},
) {
  const history = createMemoryHistory({
    initialEntries: ["/secrets?store=production"],
  });
  releases.push(() => history.destroy());
  const root = createRootRoute({
    // Deliberately no QueryClientProvider: AppShell remains presentational.
    component: () => (
      <AppShell {...props}>
        <Outlet />
      </AppShell>
    ),
    validateSearch: (search): Record<string, unknown> & { store?: string } => ({
      ...search,
      store: typeof search.store === "string" ? search.store : undefined,
    }),
  });
  const children = [
    "/setup",
    "/secrets",
    "/wrappers",
    "/stores",
    "/client-instances",
    "/users",
    "/profile",
    "/logout",
  ].map((path) =>
    createRoute({
      getParentRoute: () => root,
      path,
      component: () => <div>page</div>,
    }),
  );
  const router = createRouter({
    history,
    routeTree: root.addChildren(children),
    defaultPendingMinMs: 0,
  });
  render(<RouterProvider router={router} />);
  await screen.findByText("page");
  await waitFor(() => expect(router.state.status).toBe("idle"));
  return { history, router };
}

beforeEach(() => {
  window.localStorage.clear();
  document.documentElement.classList.add("dark");
});

afterEach(() => {
  cleanup();
  for (const release of releases.splice(0)) release();
});

describe("AppShell", () => {
  it("renders a header and main region without data-query providers", async () => {
    await renderShell();
    expect(screen.getByRole("banner")).toBeInTheDocument();
    expect(screen.getByRole("main")).toBeInTheDocument();
    expect(screen.getByText("page")).toBeInTheDocument();
    expect(
      screen.getByRole("status", { name: "Selected store context" }),
    ).toHaveTextContent("Loading store context…");
  });

  it("shows the friendly name, backend and exact lifecycle rather than asserting availability", async () => {
    await renderShell({
      store: PRODUCTION,
      storeId: PRODUCTION.id,
      storeStatus: "resolved",
    });
    const context = screen.getByRole("status", {
      name: "Selected store context",
    });
    expect(context).toHaveTextContent("Store: Production");
    expect(context).toHaveTextContent("Encrypted · locked");
    expect(context).not.toHaveTextContent(/available|ready/i);
  });

  it("hides cached lifecycle claims on error but retains requested context for destination validation", async () => {
    await renderShell({
      store: PRODUCTION,
      storeId: PRODUCTION.id,
      storeStatus: "error",
    });
    const context = screen.getByRole("status", {
      name: "Selected store context",
    });
    expect(context).toHaveTextContent("Store context could not be loaded");
    expect(context).not.toHaveTextContent(/Production|locked|available/);
    const sidebar = within(
      screen.getByRole("complementary", { name: "Sidebar" }),
    );
    expect(sidebar.getByRole("link", { name: "Stores" })).toHaveAttribute(
      "href",
      "/stores?store=production",
    );
  });

  it("preserves validated store context across sections and marks the actual active link", async () => {
    const user = userEvent.setup();
    const { history } = await renderShell({
      store: PRODUCTION,
      storeId: PRODUCTION.id,
      storeStatus: "resolved",
    });
    const sidebar = within(
      screen.getByRole("complementary", { name: "Sidebar" }),
    );
    expect(sidebar.getByRole("link", { name: "Secrets" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    for (const [name, path] of [
      ["Wrapper permissions", "/wrappers"],
      ["Stores", "/stores"],
      ["Client Instances", "/client-instances"],
      ["Get a wrapper working", "/setup"],
    ]) {
      const link = sidebar.getByRole("link", { name });
      expect(link).toHaveAttribute("href", `${path}?store=production`);
      await user.click(link);
      await waitFor(() => expect(history.location.pathname).toBe(path));
      expect(new URLSearchParams(history.location.search).get("store")).toBe(
        "production",
      );
      await waitFor(() => expect(link).toHaveAttribute("aria-current", "page"));
      expect(link).toHaveClass("aria-[current=page]:font-semibold");
    }
    expect(sidebar.getByRole("link", { name: "Secrets" })).not.toHaveAttribute(
      "aria-current",
    );
    expect(screen.getByRole("link", { name: "Sign out" })).toHaveAttribute(
      "href",
      "/logout",
    );
  });

  it("opens and closes the drawer while preserving the accessible close control", async () => {
    const user = userEvent.setup();
    await renderShell();
    await user.click(screen.getByRole("button", { name: /open navigation/i }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    const closeButton = screen.getByRole("button", { name: "Close" });
    expect(closeButton).toHaveClass(
      "focus-visible:outline-hidden",
      "focus-visible:ring-2",
      "focus-visible:ring-(--color-accent)",
    );
    expect(closeButton).not.toHaveClass("focus:outline-hidden");
    await user.click(closeButton);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("closes the drawer after navigation and carries store context", async () => {
    const user = userEvent.setup();
    const { history } = await renderShell({
      store: PRODUCTION,
      storeId: PRODUCTION.id,
      storeStatus: "resolved",
    });
    await user.click(screen.getByRole("button", { name: /open navigation/i }));
    const drawer = within(await screen.findByRole("dialog"));
    await user.click(
      drawer.getByRole("link", { name: "Get a wrapper working" }),
    );
    await waitFor(() =>
      expect(history.location.href).toBe("/setup?store=production"),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("toggles the dark theme", async () => {
    const user = userEvent.setup();
    await renderShell();
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    await user.click(
      screen.getByRole("button", { name: /switch to light theme/i }),
    );
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    expect(window.localStorage.getItem("uw.theme")).toBe("light");
    await user.click(
      screen.getByRole("button", { name: /switch to dark theme/i }),
    );
    expect(document.documentElement.classList.contains("dark")).toBe(true);
  });
});
