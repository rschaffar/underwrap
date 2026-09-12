import { beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { withTestRouter } from "@/test-router";
import { StoreNavigation } from "@/components/store-navigation";
import {
  fetchStores,
  fetchClientInstances,
  fetchSecretsView,
  fetchWrapperPermissionsView,
  fetchLibsecretTree,
  type AdminStoresListResponse,
} from "@/api/client";
import { SecretsPage } from "./secrets/page";
import { WrapperPermissionsPage } from "./wrapper-permissions/page";
import { StoresPage } from "./stores/page";
import { ClientInstancesPage } from "./client-instances/page";
import { SetupPage } from "./setup";

vi.mock("@/api/client", async () => ({
  ...(await vi.importActual<typeof import("@/api/client")>("@/api/client")),
  fetchStores: vi.fn(),
  fetchClientInstances: vi.fn(),
  fetchSecretsView: vi.fn(),
  fetchWrapperPermissionsView: vi.fn(),
  fetchLibsecretTree: vi.fn(),
}));

function store(
  id: string,
  locked = false,
): AdminStoresListResponse["items"][number] {
  return {
    id,
    name: id === "primary" ? "Development" : "Production",
    backend: locked ? "encrypted" : "plain",
    lifecycle_state: locked ? "locked" : "ready",
    usage: { mapped_client_instance_count: 0, has_secrets: false },
    actions: {
      can_update: true,
      can_delete_now: true,
      can_force_delete: false,
      can_migrate: true,
      can_rotate_key: false,
      can_unlock: locked,
      can_lock: false,
    },
  };
}
function resource(id: string, name: string) {
  return {
    id,
    name,
    created_at: 1,
    created_by: {
      kind: "operator" as const,
      id: "operator",
      username: "operator",
    },
  };
}
function renderPage(node: ReactNode, path: string) {
  window.history.replaceState(null, "", path);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      {withTestRouter(<StoreNavigation>{node}</StoreNavigation>)}
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.spyOn(globalThis, "fetch").mockRejectedValue(
    new Error("unexpected non-metadata request"),
  );
  vi.mocked(fetchStores).mockResolvedValue({
    items: [store("primary"), store("secondary")],
  });
  vi.mocked(fetchClientInstances).mockResolvedValue({ items: [] });
  vi.mocked(fetchSecretsView).mockResolvedValue({ entries: [], resources: [] });
  vi.mocked(fetchWrapperPermissionsView).mockResolvedValue({
    permissions: [],
    pairs: [],
    resources: [],
  });
  vi.mocked(fetchLibsecretTree).mockResolvedValue({
    aliases: {},
    collections: [],
  });
});

describe("operator workflow", () => {
  it.each([
    ["/secrets", "resource", <SecretsPage />],
    ["/wrappers", "pair", <WrapperPermissionsPage />],
    ["/client-instances", "client", <ClientInstancesPage />],
  ])(
    "does not move %s creation to a fallback store",
    async (path, intent, page) => {
      renderPage(
        page,
        `${path}?store=removed&create=${intent}&keep=yes#section`,
      );
      await waitFor(() =>
        expect(window.location.search).toBe("?store=primary&keep=yes"),
      );
      expect(window.location.hash).toBe("#section");
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      expect(globalThis.fetch).not.toHaveBeenCalled();
    },
  );

  it("opens enrollment after real navigation with already cached metadata", async () => {
    window.history.replaceState(null, "", "/setup?store=secondary");
    const qc = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    qc.setQueryData(["admin", "stores"], {
      items: [store("primary"), store("secondary")],
    });
    qc.setQueryData(["admin", "client-instances"], { items: [] });
    render(
      <QueryClientProvider client={qc}>
        {withTestRouter(
          <StoreNavigation>
            <SetupPage />
          </StoreNavigation>,
          {
            "/client-instances": (
              <StoreNavigation>
                <ClientInstancesPage />
              </StoreNavigation>
            ),
          },
        )}
      </QueryClientProvider>,
    );
    await userEvent
      .setup()
      .click(
        await screen.findByRole("link", { name: "Enroll client instance" }),
      );
    expect(
      await screen.findByRole("dialog", { name: "Create client instance" }),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(window.location.search).toBe("?store=secondary"),
    );
    expect(globalThis.fetch).not.toHaveBeenCalled();
  });

  it("consumes an exact valid create intent without submitting it", async () => {
    renderPage(<SecretsPage />, "/secrets?store=secondary&create=resource");
    expect(
      await screen.findByRole("dialog", { name: /create resource/i }),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(window.location.search).toBe("?store=secondary"),
    );
    expect(globalThis.fetch).not.toHaveBeenCalled();
  });

  it.each(["secondary", "missing", "primary"])(
    "validates unlock=%s independently of selected-store fallback",
    async (target) => {
      vi.mocked(fetchStores).mockResolvedValue({
        items: [store("primary"), store("secondary", true)],
      });
      renderPage(<StoresPage />, `/stores?store=primary&unlock=${target}`);
      await waitFor(() =>
        expect(window.location.search).toBe("?store=primary"),
      );
      if (target === "secondary")
        expect(
          screen.getByRole("dialog", { name: /unlock.*production/i }),
        ).toBeInTheDocument();
      else expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      expect(globalThis.fetch).not.toHaveBeenCalled();
    },
  );

  it("filters resource and entry metadata without reading values or unopened libsecret trees", async () => {
    vi.mocked(fetchSecretsView).mockResolvedValue({
      resources: [
        resource("res_first", "First resource"),
        resource("res_second", "Second resource"),
      ],
      entries: [{ resource_id: "res_first", slot: "env", key: "CONFIG_NAME" }],
    });
    renderPage(<SecretsPage />, "/secrets?store=secondary");
    expect(await screen.findByText("First resource")).toBeInTheDocument();
    expect(screen.getByText("Second resource")).toBeInTheDocument();
    expect(screen.getByText("No env/files entries")).toBeInTheDocument();
    fireEvent.change(
      screen.getByRole("searchbox", {
        name: "Filter resources and entry names",
      }),
      { target: { value: "config_name" } },
    );
    expect(screen.getByText("First resource")).toBeInTheDocument();
    expect(screen.queryByText("Second resource")).not.toBeInTheDocument();
    expect(screen.getByText(/1 of 2 resources/)).toBeInTheDocument();
    expect(fetchLibsecretTree).not.toHaveBeenCalled();
    expect(globalThis.fetch).not.toHaveBeenCalled();
  });

  it.each([false, true])(
    "guides explicit auto-create=%s without claiming restart or target success",
    async (autoCreate) => {
      vi.mocked(fetchClientInstances).mockResolvedValue({
        items: [
          {
            id: "client_demo",
            display_name: "Backend display name",
            current_store_id: "secondary",
            current_store_name: "Production",
            current_credential_fingerprint: "synthetic-metadata",
            status: "active",
            auto_create_wrapper_assets: autoCreate,
            access_override: { kind: "individual" },
            actions: { can_disable: true, can_enable: false },
          },
        ],
      });
      renderPage(<SetupPage />, "/setup?store=secondary");
      await screen.findByRole("option", { name: /backend display name/i });
      expect(
        screen.getByText(/backend display name is not/i),
      ).toBeInTheDocument();
      expect(
        screen.getAllByText(/not observed/i).length,
      ).toBeGreaterThanOrEqual(1);
      expect(
        screen.getByText(
          autoCreate
            ? /asset auto-creation is explicitly enabled/i
            : /asset auto-creation is off/i,
        ),
      ).toBeInTheDocument();
      expect(globalThis.fetch).not.toHaveBeenCalled();
    },
  );
});
