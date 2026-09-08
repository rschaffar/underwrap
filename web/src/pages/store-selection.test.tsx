import { StrictMode, type ReactElement } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import {
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const dialogStoreBindings = vi.hoisted(() => ({
  secrets: [] as string[],
  wrappers: [] as string[],
}));

vi.mock("@/api/client", async () => {
  const actual =
    await vi.importActual<typeof import("@/api/client")>("@/api/client");
  return {
    ...actual,
    fetchStores: vi.fn(),
    fetchSecretsView: vi.fn(),
    fetchWrapperPermissionsView: vi.fn(),
    fetchClientInstances: vi.fn(),
    fetchLibsecretTree: vi.fn(),
    deleteLibsecretCollection: vi.fn(),
  };
});

vi.mock("./secrets/dialogs/create-resource", () => ({
  CreateResourceDialog: ({ storeId }: { storeId: string }) => {
    dialogStoreBindings.secrets.push(storeId);
    return (
      <div role="dialog" aria-label="Create resource">
        {storeId}
      </div>
    );
  },
}));

vi.mock("./wrapper-permissions/dialogs/create-pair", () => ({
  CreatePairDialog: ({ storeId }: { storeId: string }) => {
    dialogStoreBindings.wrappers.push(storeId);
    return (
      <div role="dialog" aria-label="Create wrapper/resource pair">
        {storeId}
      </div>
    );
  },
}));

import {
  type AdminLibsecretTreeResponse,
  type AdminStoresListResponse,
  deleteLibsecretCollection,
  fetchClientInstances,
  fetchLibsecretTree,
  fetchSecretsView,
  fetchStores,
  fetchWrapperPermissionsView,
} from "@/api/client";
import { SecretsPage } from "./secrets/page";
import { WrapperPermissionsPage } from "./wrapper-permissions/page";

const STORE_LIST = {
  items: [
    {
      id: "primary",
      name: "Primary",
      backend: "plain" as const,
      created_at: 1,
      lifecycle_state: "ready" as const,
      usage: { mapped_client_instance_count: 0, has_secrets: false },
      actions: {
        can_update: true,
        can_migrate: true,
        can_rotate_key: false,
        can_unlock: false,
        can_lock: false,
        can_delete_now: true,
        can_force_delete: false,
      },
    },
    {
      id: "secondary",
      name: "Secondary",
      backend: "plain" as const,
      created_at: 2,
      lifecycle_state: "ready" as const,
      usage: { mapped_client_instance_count: 0, has_secrets: false },
      actions: {
        can_update: true,
        can_migrate: true,
        can_rotate_key: false,
        can_unlock: false,
        can_lock: false,
        can_delete_now: true,
        can_force_delete: false,
      },
    },
  ],
} satisfies AdminStoresListResponse;

const PRIMARY_ONLY = { items: [STORE_LIST.items[0]] } satisfies AdminStoresListResponse;

const SHARED_RESOURCE = {
  id: "shared-resource",
  name: "Shared resource",
  created_by: { kind: "operator" as const, id: "operator", username: "alice" },
  created_at: 1,
};

function libsecretTree(label: string): AdminLibsecretTreeResponse {
  return {
    aliases: {},
    collections: [
      {
        id: "shared-collection",
        label,
        created: 1,
        modified: 1,
        items: [],
      },
    ],
  };
}

const PAGE_CASES = [
  {
    name: "Secrets",
    path: "/secrets",
    ui: <SecretsPage />,
    selectTestId: "secrets-store-select",
    createButton: /create resource/i,
    createDialog: /create resource/i,
    contentEmpty: /no resources are known to this store yet/i,
    bindings: dialogStoreBindings.secrets,
  },
  {
    name: "Wrapper permissions",
    path: "/wrappers",
    ui: <WrapperPermissionsPage />,
    selectTestId: "wrappers-store-select",
    createButton: /create pair/i,
    createDialog: /create wrapper\/resource pair/i,
    contentEmpty: /no wrapper\/resource pairs are registered/i,
    bindings: dialogStoreBindings.wrappers,
  },
] as const;

function detailMock(name: (typeof PAGE_CASES)[number]["name"]) {
  return name === "Secrets"
    ? vi.mocked(fetchSecretsView)
    : vi.mocked(fetchWrapperPermissionsView);
}

function renderPage(ui: ReactElement): QueryClient {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>
    </StrictMode>,
  );
  return queryClient;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function setLocation(path: string) {
  window.history.replaceState(null, "", path);
}

function expandResourceBlock(block: HTMLElement) {
  block.setAttribute("open", "");
  fireEvent(block, new Event("toggle", { bubbles: false }));
}

beforeEach(() => {
  vi.mocked(fetchStores).mockReset();
  vi.mocked(fetchSecretsView).mockReset();
  vi.mocked(fetchWrapperPermissionsView).mockReset();
  vi.mocked(fetchClientInstances).mockReset();
  vi.mocked(fetchLibsecretTree).mockReset();
  vi.mocked(deleteLibsecretCollection).mockReset();
  vi.mocked(fetchClientInstances).mockResolvedValue({ items: [] });
  vi.mocked(fetchSecretsView).mockResolvedValue({ entries: [], resources: [] });
  vi.mocked(fetchLibsecretTree).mockResolvedValue({ aliases: {}, collections: [] });
  vi.mocked(fetchWrapperPermissionsView).mockResolvedValue({
    permissions: [],
    pairs: [],
    resources: [],
  });
  dialogStoreBindings.secrets.length = 0;
  dialogStoreBindings.wrappers.length = 0;
});

afterEach(() => {
  setLocation("/");
});

describe.each(PAGE_CASES)("$name store selection", (pageCase) => {
  it("blocks stale detail work until the pending list resolves to the first valid store", async () => {
    const stores = deferred<AdminStoresListResponse>();
    vi.mocked(fetchStores).mockReturnValue(stores.promise);
    setLocation(`${pageCase.path}?store=stale&keep=yes#section`);

    renderPage(pageCase.ui);
    expect(detailMock(pageCase.name)).not.toHaveBeenCalled();

    stores.resolve(STORE_LIST);

    await waitFor(() =>
      expect(screen.getByTestId(pageCase.selectTestId)).toHaveValue("primary"),
    );
    await waitFor(() => expect(detailMock(pageCase.name)).toHaveBeenCalled());
    expect(detailMock(pageCase.name).mock.calls.every(([id]) => id === "primary")).toBe(
      true,
    );
    expect(window.location.pathname + window.location.search + window.location.hash).toBe(
      `${pageCase.path}?store=primary&keep=yes#section`,
    );
  });

  it("shows a store-list network error without changing the requested URL or diagnosing access/content", async () => {
    vi.mocked(fetchStores).mockRejectedValue(new Error("offline"));
    setLocation(`${pageCase.path}?store=requested&keep=yes#section`);

    renderPage(pageCase.ui);

    expect(
      await screen.findByText("Network error. Check your connection and try again."),
    ).toBeInTheDocument();
    expect(window.location.pathname + window.location.search + window.location.hash).toBe(
      `${pageCase.path}?store=requested&keep=yes#section`,
    );
    expect(screen.queryByText(/grant/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/ask an admin for access/i)).not.toBeInTheDocument();
    expect(screen.queryByText(pageCase.contentEmpty)).not.toBeInTheDocument();
    expect(detailMock(pageCase.name)).not.toHaveBeenCalled();
  });

  it("clears only store for a successful empty list and shows create-store guidance", async () => {
    vi.mocked(fetchStores).mockResolvedValue({ items: [] });
    setLocation(`${pageCase.path}?store=requested&keep=yes#section`);

    renderPage(pageCase.ui);

    expect(
      await screen.findByText("No stores are configured. Create one on the Stores page."),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(
        window.location.pathname + window.location.search + window.location.hash,
      ).toBe(`${pageCase.path}?keep=yes#section`),
    );
    expect(detailMock(pageCase.name)).not.toHaveBeenCalled();
  });

  it("synchronously suppresses and resets an open create dialog when the selected store is removed", async () => {
    vi.mocked(fetchStores).mockResolvedValue(STORE_LIST);
    setLocation(`${pageCase.path}?store=secondary&keep=yes#section`);
    const queryClient = renderPage(pageCase.ui);

    await waitFor(() =>
      expect(screen.getByTestId(pageCase.selectTestId)).toHaveValue("secondary"),
    );
    await waitFor(() =>
      expect(detailMock(pageCase.name)).toHaveBeenCalledWith("secondary"),
    );

    fireEvent.click(screen.getByRole("button", { name: pageCase.createButton }));
    expect(
      screen.getByRole("dialog", { name: pageCase.createDialog }),
    ).toBeInTheDocument();
    expect(pageCase.bindings.length).toBeGreaterThan(0);
    expect(pageCase.bindings.every((id) => id === "secondary")).toBe(true);

    pageCase.bindings.length = 0;
    detailMock(pageCase.name).mockClear();
    act(() => {
      queryClient.setQueryData(["admin", "stores"], PRIMARY_ONLY);
    });

    await waitFor(() =>
      expect(screen.getByTestId(pageCase.selectTestId)).toHaveValue("primary"),
    );
    expect(
      screen.queryByRole("dialog", { name: pageCase.createDialog }),
    ).not.toBeInTheDocument();
    await act(async () => {});
    expect(pageCase.bindings).toEqual([]);
    expect(window.location.pathname + window.location.search + window.location.hash).toBe(
      `${pageCase.path}?store=primary&keep=yes#section`,
    );
    await waitFor(() =>
      expect(detailMock(pageCase.name)).toHaveBeenCalledWith("primary"),
    );
    expect(
      detailMock(pageCase.name).mock.calls.every(([id]) => id === "primary"),
    ).toBe(true);

    pageCase.bindings.length = 0;
    fireEvent.click(screen.getByRole("button", { name: pageCase.createButton }));
    expect(
      screen.getByRole("dialog", { name: pageCase.createDialog }),
    ).toBeInTheDocument();
    expect(pageCase.bindings.length).toBeGreaterThan(0);
    expect(pageCase.bindings.every((id) => id === "primary")).toBe(true);
  });
});

it("remounts Secrets resource state before cached same-id libsecret data can rebind across stores", async () => {
  const primaryView = { entries: [], resources: [SHARED_RESOURCE] };
  const secondaryView = { entries: [], resources: [SHARED_RESOURCE] };
  const primaryTree = libsecretTree("Primary collection");
  const secondaryTree = libsecretTree("Secondary collection");
  vi.mocked(fetchStores).mockResolvedValue(STORE_LIST);
  vi.mocked(fetchSecretsView).mockImplementation(async (storeId) =>
    storeId === "secondary" ? secondaryView : primaryView,
  );
  vi.mocked(fetchLibsecretTree).mockImplementation(async (storeId) =>
    storeId === "secondary" ? secondaryTree : primaryTree,
  );
  setLocation("/secrets?store=secondary&keep=yes#section");
  const queryClient = renderPage(<SecretsPage />);

  const sourceBlock = await screen.findByTestId(
    "resource-block-shared-resource",
  );
  expandResourceBlock(sourceBlock);
  expect(
    await within(sourceBlock).findByText("Secondary collection"),
  ).toBeInTheDocument();
  fireEvent.click(
    within(sourceBlock).getByRole("button", {
      name: "Delete collection Secondary collection",
    }),
  );
  expect(
    await screen.findByRole("dialog", { name: "Delete collection" }),
  ).toBeInTheDocument();

  act(() => {
    queryClient.setQueryData(
      ["admin", "secrets-view", "primary"],
      primaryView,
    );
    queryClient.setQueryData(
      ["admin", "libsecret", "primary", "shared-resource"],
      primaryTree,
    );
    queryClient.setQueryData(["admin", "stores"], PRIMARY_ONLY);
  });

  await waitFor(() =>
    expect(screen.getByTestId("secrets-store-select")).toHaveValue("primary"),
  );
  expect(
    screen.queryByRole("dialog", { name: "Delete collection" }),
  ).not.toBeInTheDocument();
  expect(deleteLibsecretCollection).not.toHaveBeenCalled();

  const destinationBlock = screen.getByTestId(
    "resource-block-shared-resource",
  );
  expect(destinationBlock).not.toHaveAttribute("open");
  expect(
    within(destinationBlock).queryByText("Secondary collection"),
  ).not.toBeInTheDocument();

  expandResourceBlock(destinationBlock);
  expect(
    await within(destinationBlock).findByText("Primary collection"),
  ).toBeInTheDocument();
  expect(
    within(destinationBlock).queryByText("Secondary collection"),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByRole("dialog", { name: "Delete collection" }),
  ).not.toBeInTheDocument();
  expect(deleteLibsecretCollection).not.toHaveBeenCalled();
});
