import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  fetchStores,
  fetchClientInstances,
  fetchWrapperPermissionsView,
  putClientInstanceUpdate,
  putWrapperPermission,
  deleteWrapperPermission,
  type AdminClientInstanceListItem,
  type AdminStoreListItem,
  type AdminWrapperPermissionsView,
  type WrapperApprovalMode,
} from "@/api/client";
import { withTestRouter } from "@/test-router";
import { ClientInstancesPage } from "../client-instances/page";
import { WrapperPermissionsPage } from "./page";

vi.mock("@/api/client", async () => ({
  ...(await vi.importActual<typeof import("@/api/client")>("@/api/client")),
  fetchStores: vi.fn(),
  fetchClientInstances: vi.fn(),
  fetchWrapperPermissionsView: vi.fn(),
  putClientInstanceUpdate: vi.fn(),
  putWrapperPermission: vi.fn(),
  deleteWrapperPermission: vi.fn(),
}));

type Override = AdminClientInstanceListItem["access_override"];
function client(
  id: string,
  access_override: Override = { kind: "individual" },
): AdminClientInstanceListItem {
  return {
    id,
    display_name: `${id} runtime`,
    current_store_id: "production",
    current_store_name: "Production",
    current_credential_fingerprint: "synthetic-fingerprint",
    status: "active",
    auto_create_wrapper_assets: false,
    access_override,
    actions: { can_disable: true, can_enable: false },
  };
}
const store: AdminStoreListItem = {
  id: "production",
  name: "Production",
  backend: "plain",
  lifecycle_state: "ready",
  usage: { mapped_client_instance_count: 2, has_secrets: false },
  actions: {
    can_update: true,
    can_delete_now: false,
    can_force_delete: false,
    can_migrate: true,
    can_rotate_key: false,
    can_unlock: false,
    can_lock: false,
  },
};
let clients: AdminClientInstanceListItem[];
let view: AdminWrapperPermissionsView;
const queryClients: QueryClient[] = [];
const creator = {
  kind: "operator" as const,
  id: "operator",
  username: "operator",
};
function renderPage(
  node: ReactNode = <WrapperPermissionsPage />,
  path = "/wrappers?store=production",
) {
  window.history.replaceState(null, "", path);
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  queryClients.push(qc);
  const rendered = render(
    <QueryClientProvider client={qc}>
      {withTestRouter(node)}
    </QueryClientProvider>,
  );
  return { ...rendered, qc };
}
const header = (id = "Alpha") =>
  screen.getByTestId(`wrappers-client-column-${id}`);
const controls = (id = "Alpha") =>
  within(header(id)).getByRole("group", {
    name: `Access override for ${id} runtime`,
  });
const selector = (id = "Alpha") =>
  within(controls(id)).getByLabelText("Client access");
const cell = (id = "Alpha") =>
  screen.getByTestId(`wrappers-cell-wrapper-resource-${id}`);
async function ready() {
  await screen.findByTestId("wrappers-cell-wrapper-resource-Alpha");
  await waitFor(() => expect(selector()).toBeEnabled());
}

beforeEach(() => {
  vi.resetAllMocks();
  clients = [client("Alpha"), client("Beta")];
  view = {
    resources: [
      { id: "resource", name: "Resource", created_at: 1, created_by: creator },
    ],
    pairs: [
      {
        wrapper: "wrapper",
        resource_id: "resource",
        created_at: 1,
        created_by: creator,
      },
    ],
    permissions: clients.map((item) => ({
      client_instance: item.id,
      wrapper: "wrapper",
      resource_id: "resource",
      mode: { kind: "unlocked" },
      writable: true,
      reason: "saved individual reason",
    })),
  };
  vi.mocked(fetchStores).mockResolvedValue({ items: [store] });
  // Model fresh JSON snapshots; mutating fixture state must not mutate query caches.
  vi.mocked(fetchClientInstances).mockImplementation(async () => ({
    items: structuredClone(clients),
  }));
  vi.mocked(fetchWrapperPermissionsView).mockImplementation(async () =>
    structuredClone(view),
  );
  vi.mocked(putClientInstanceUpdate).mockImplementation(async (id, body) => {
    const current = clients.find((item) => item.id === id)!;
    const updated = {
      ...current,
      access_override: body.access_override ?? current.access_override,
    };
    clients = clients.map((item) => (item.id === id ? updated : item));
    return updated;
  });
});

afterEach(() => {
  expect(putWrapperPermission).not.toHaveBeenCalled();
  expect(deleteWrapperPermission).not.toHaveBeenCalled();
  for (const qc of queryClients.splice(0)) qc.clear();
});

describe("instance controls in permission columns", () => {
  it("places controls below the correct heading and changes only that column after acknowledgement", async () => {
    const saved = structuredClone(view.permissions);
    let finish!: () => void;
    vi.mocked(putClientInstanceUpdate).mockImplementationOnce(
      async (id, body) => {
        await new Promise<void>((resolve) => {
          finish = resolve;
        });
        const updated = {
          ...clients.find((item) => item.id === id)!,
          access_override: body.access_override!,
        };
        clients = clients.map((item) => (item.id === id ? updated : item));
        return updated;
      },
    );
    renderPage();
    await ready();
    const link = within(header()).getByRole("link", { name: "Alpha runtime" });
    expect(
      link.compareDocumentPosition(selector()) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(selector()).toHaveValue("individual");
    expect(within(cell()).getByTestId("mode-unlocked")).toHaveAttribute(
      "data-muted",
      "false",
    );
    const user = userEvent.setup();
    await user.selectOptions(selector(), "locked");
    expect(putClientInstanceUpdate).toHaveBeenCalledExactlyOnceWith("Alpha", {
      access_override: { kind: "locked" },
    });
    expect(selector()).toBeDisabled();
    expect(cell()).toHaveAttribute("data-overridden", "false");
    await act(async () => finish());
    await waitFor(() =>
      expect(cell()).toHaveAttribute("data-overridden", "true"),
    );
    expect(cell()).toHaveClass("border-dashed");
    expect(within(cell()).getByTestId("mode-unlocked")).toHaveAttribute(
      "data-muted",
      "true",
    );
    expect(cell()).toHaveTextContent("Overridden · Lock all");
    expect(cell()).toHaveTextContent("Saved writeback: on");
    expect(cell("Beta")).toHaveAttribute("data-overridden", "false");
    expect(selector("Beta")).toHaveValue("individual");
    expect(view.permissions).toEqual(saved);

    // The saved row is still editable, including its original flag.
    await user.click(cell());
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByRole("checkbox")).toBeChecked();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await user.selectOptions(selector(), "individual");
    await waitFor(() =>
      expect(cell()).toHaveAttribute("data-overridden", "false"),
    );
    expect(within(cell()).getByTestId("mode-unlocked")).toHaveAttribute(
      "data-muted",
      "false",
    );
    expect(cell()).not.toHaveClass("border-dashed");
    expect(view.permissions).toEqual(saved);
  });

  it("does not mute on an unconfirmed unlock, then uses the override's writable flag without rewriting the row", async () => {
    renderPage();
    await ready();
    const user = userEvent.setup();
    await user.selectOptions(selector(), "unlocked");
    let confirmation = screen.getByRole("dialog", {
      name: "Unlock all: Alpha runtime",
    });
    expect(putClientInstanceUpdate).not.toHaveBeenCalled();
    expect(cell()).toHaveAttribute("data-overridden", "false");
    await user.click(
      within(confirmation).getByRole("button", { name: "Cancel" }),
    );
    await user.selectOptions(selector(), "unlocked");
    confirmation = screen.getByRole("dialog", {
      name: "Unlock all: Alpha runtime",
    });
    await user.click(
      within(confirmation).getByRole("button", {
        name: "Unlock all read-only",
      }),
    );
    await waitFor(() =>
      expect(cell()).toHaveTextContent("Overridden · Unlock all"),
    );
    expect(within(controls()).getByRole("checkbox")).not.toBeChecked();
    expect(cell()).toHaveTextContent("Saved writeback: on");
    await user.click(within(controls()).getByRole("checkbox"));
    await waitFor(() =>
      expect(within(controls()).getByRole("checkbox")).toBeChecked(),
    );
    expect(putClientInstanceUpdate).toHaveBeenLastCalledWith("Alpha", {
      access_override: { kind: "unlocked", writable: true },
    });
    expect(view.permissions[0].mode).toEqual({ kind: "unlocked" });
    expect(view.permissions[0].writable).toBe(true);
  });

  it("marks an ambiguous update unknown rather than retaining the previous override's visual claim", async () => {
    clients[0] = client("Alpha", { kind: "locked" });
    renderPage();
    await ready();
    vi.mocked(putClientInstanceUpdate).mockRejectedValueOnce(
      new Error("response lost"),
    );
    vi.mocked(fetchClientInstances).mockRejectedValue(
      new Error("refresh unavailable"),
    );
    const user = userEvent.setup();
    await user.selectOptions(selector(), "individual");
    await waitFor(() => expect(selector()).toHaveValue("unknown"));
    expect(cell()).toHaveTextContent("Override unknown");
    expect(cell()).not.toHaveTextContent("Overridden · Lock all");
    expect(cell()).toHaveAttribute("data-overridden", "false");
    fireEvent.change(screen.getByLabelText("Filter client columns"), {
      target: { value: "Beta" },
    });
    expect(
      screen.queryByRole("group", {
        name: "Access override for Alpha runtime",
      }),
    ).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Filter client columns"), {
      target: { value: "" },
    });
    expect(selector()).toHaveValue("unknown");
    expect(selector()).toBeDisabled();
    expect(cell()).toHaveTextContent("Override unknown");
    await user.click(cell());
    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).queryByText(/Lock all currently denies/),
    ).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    clients[0] = client("Alpha", { kind: "unlocked", writable: false });
    vi.mocked(fetchClientInstances).mockResolvedValue({ items: clients });
    await user.click(
      within(controls()).getByRole("button", { name: "Retry refresh" }),
    );
    await waitFor(() => expect(selector()).toHaveValue("unlocked"));
    expect(cell()).toHaveTextContent("Overridden · Unlock all");
    expect(cell()).not.toHaveTextContent("Override unknown");
  });

  it("retains an in-flight owner when filtering away a column before its response is lost", async () => {
    let reject!: (error: Error) => void;
    vi.mocked(putClientInstanceUpdate).mockReturnValue(
      new Promise((_, rejectRequest) => {
        reject = rejectRequest;
      }),
    );
    renderPage();
    await ready();
    vi.mocked(fetchClientInstances).mockRejectedValue(
      new Error("refresh unavailable"),
    );
    await userEvent.setup().selectOptions(selector(), "locked");
    fireEvent.change(screen.getByLabelText("Filter client columns"), {
      target: { value: "Beta" },
    });
    expect(
      screen.queryByRole("group", {
        name: "Access override for Alpha runtime",
      }),
    ).not.toBeInTheDocument();
    await act(async () => reject(new Error("response lost")));
    fireEvent.change(screen.getByLabelText("Filter client columns"), {
      target: { value: "" },
    });
    await waitFor(() => expect(selector()).toHaveValue("unknown"));
    expect(selector()).toBeDisabled();
    expect(cell()).toHaveTextContent("Override unknown");
    expect(
      within(controls()).getByRole("button", { name: "Retry refresh" }),
    ).toBeEnabled();
  });

  it.each(["empty", "filtered", "loading", "error", "locked-store"])(
    "retains mapped instance controls with %s permission contents",
    async (state) => {
      if (state === "empty") view.pairs = [];
      if (state === "loading")
        vi.mocked(fetchWrapperPermissionsView).mockReturnValue(
          new Promise(() => {}),
        );
      if (state === "error")
        vi.mocked(fetchWrapperPermissionsView).mockRejectedValue(
          new Error("view unavailable"),
        );
      if (state === "locked-store")
        vi.mocked(fetchStores).mockResolvedValue({
          items: [
            { ...store, backend: "encrypted", lifecycle_state: "locked" },
          ],
        });
      renderPage();
      await waitFor(() => expect(selector()).toBeEnabled());
      if (state === "filtered") {
        await screen.findByTestId("wrappers-cell-wrapper-resource-Alpha");
        fireEvent.change(
          screen.getByLabelText("Filter wrappers and resources"),
          { target: { value: "no match" } },
        );
        expect(
          screen.getByText("No pairs match this metadata filter."),
        ).toBeInTheDocument();
      }
      expect(
        screen.queryByTestId("wrappers-cell-wrapper-resource-Alpha"),
      ).not.toBeInTheDocument();
      if (state !== "loading" && state !== "error") {
        await userEvent.setup().selectOptions(selector(), "locked");
        await waitFor(() => expect(selector()).toHaveValue("locked"));
        expect(putClientInstanceUpdate).toHaveBeenCalledExactlyOnceWith(
          "Alpha",
          { access_override: { kind: "locked" } },
        );
      }
      if (state === "locked-store") {
        expect(fetchWrapperPermissionsView).not.toHaveBeenCalled();
        const filter = screen.getByLabelText("Filter client columns");
        expect(filter).toBeEnabled();
        fireEvent.change(filter, { target: { value: "Beta" } });
        expect(selector("Beta")).toBeEnabled();
        expect(
          screen.queryByRole("group", {
            name: "Access override for Alpha runtime",
          }),
        ).not.toBeInTheDocument();
      }
      if (
        state === "locked-store" ||
        state === "error" ||
        state === "loading"
      ) {
        expect(
          screen.getByTestId("writable-permissions-badge"),
        ).toHaveTextContent("Saved flags unavailable");
      }
    },
  );

  it("does not offer controls for stale permissions from an unmapped instance", async () => {
    clients[0] = {
      ...client("Alpha", { kind: "locked" }),
      current_store_id: "other",
    };
    renderPage();
    await screen.findByTestId("wrappers-cell-wrapper-resource-Alpha");
    await waitFor(() =>
      expect(header()).toHaveTextContent("No current mapping to this store"),
    );
    expect(within(header()).queryByRole("group")).not.toBeInTheDocument();
    expect(cell()).toHaveAttribute("data-overridden", "false");
    expect(selector("Beta")).toBeEnabled();
  });

  it("hides stale control and override claims when client metadata refresh fails", async () => {
    clients[0] = client("Alpha", { kind: "locked" });
    const rendered = renderPage();
    await ready();
    expect(cell()).toHaveAttribute("data-overridden", "true");
    vi.mocked(fetchClientInstances).mockRejectedValue(
      new Error("client metadata unavailable"),
    );
    await act(async () => {
      await rendered.qc.refetchQueries({
        queryKey: ["admin", "client-instances"],
      });
    });
    await waitFor(() =>
      expect(within(header()).queryByRole("group")).not.toBeInTheDocument(),
    );
    expect(header()).toHaveTextContent("Client state unavailable");
    expect(cell()).toHaveAttribute("data-overridden", "false");
    expect(cell()).not.toHaveTextContent("Overridden · Lock all");
  });

  it("keeps the client inventory as identity management with a mapped-store link, not duplicate controls", async () => {
    renderPage(<ClientInstancesPage />, "/client-instances?store=production");
    await screen.findByTestId("client-instance-row-Alpha");
    expect(
      screen.queryByRole("group", { name: /Access override for/ }),
    ).not.toBeInTheDocument();
    expect(
      within(screen.getByTestId("client-instance-row-Alpha")).getByRole(
        "link",
        { name: "Wrapper permissions" },
      ),
    ).toHaveAttribute("href", "/wrappers?store=production");
  });
});

const modes: Array<[WrapperApprovalMode, string]> = [
  [{ kind: "locked" }, "mode-locked"],
  [{ kind: "unlocked" }, "mode-unlocked"],
  [
    { kind: "unlocked-until", until_epoch: 4_000_000_000 },
    "mode-unlocked-until",
  ],
  [{ kind: "unlocked-until", until_epoch: 1 }, "mode-expired"],
  [
    { kind: "unsafe-debug-until", until_epoch: 4_000_000_000 },
    "mode-unsafe-debug-until",
  ],
  [{ kind: "unsafe-debug-until", until_epoch: 1 }, "mode-unsafe-debug-expired"],
];
describe.each(["locked", "unlocked"] as const)(
  "saved badges under %s override",
  (kind) => {
    it.each(modes)(
      "preserves %j and its expiry while muting only overridden modes",
      async (mode, testId) => {
        clients[0] = client(
          "Alpha",
          kind === "locked" ? { kind } : { kind, writable: false },
        );
        view.permissions[0] = { ...view.permissions[0], mode };
        renderPage();
        await ready();
        const debugStillApplies =
          kind === "unlocked" && mode.kind === "unsafe-debug-until";
        expect(cell()).toHaveAttribute("data-overridden", "true");
        expect(within(cell()).getByTestId(testId)).toHaveAttribute(
          "data-muted",
          String(!debugStillApplies),
        );
        expect(cell()).toHaveTextContent("Saved writeback: on");
        if (debugStillApplies)
          expect(cell()).toHaveTextContent(
            "Debug approval and expiry still apply",
          );
        expect(view.permissions[0].mode).toEqual(mode);
        expect(cell()).toBeEnabled();
      },
    );
    it("does not fabricate a saved row for an overridden absent permission", async () => {
      clients[0] = client(
        "Alpha",
        kind === "locked" ? { kind } : { kind, writable: false },
      );
      view.permissions = view.permissions.filter(
        (permission) => permission.client_instance !== "Alpha",
      );
      renderPage();
      await ready();
      expect(within(cell()).getByTestId("mode-missing")).toBeInTheDocument();
      expect(cell()).toHaveAttribute("data-overridden", "true");
      expect(cell()).not.toHaveTextContent("Saved writeback:");
      expect(view.permissions).toHaveLength(1);
    });
  },
);
