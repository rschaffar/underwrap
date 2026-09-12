import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  fetchStores,
  fetchClientInstances,
  fetchSecretsView,
  fetchWrapperPermissionsView,
  postClientInstanceCreate,
  type AdminStoreListItem,
  type AdminClientInstanceListItem,
} from "@/api/client";
import { withTestRouter } from "@/test-router";
import { SetupPage } from "./setup";
import { SecretsPage } from "./secrets/page";
import { ClientInstancesPage } from "./client-instances/page";
import { CreateClientInstanceDialog } from "./client-instances/dialogs/create";

vi.mock("@/api/client", async () => ({
  ...(await vi.importActual<typeof import("@/api/client")>("@/api/client")),
  fetchStores: vi.fn(),
  fetchClientInstances: vi.fn(),
  fetchSecretsView: vi.fn(),
  fetchWrapperPermissionsView: vi.fn(),
  postClientInstanceCreate: vi.fn(),
}));

function store(id: string): AdminStoreListItem {
  return {
    id,
    name: id,
    backend: "plain",
    lifecycle_state: "ready",
    usage: { mapped_client_instance_count: 0, has_secrets: false },
    actions: {
      can_update: true,
      can_delete_now: true,
      can_force_delete: false,
      can_migrate: true,
      can_rotate_key: false,
      can_unlock: false,
      can_lock: false,
    },
  };
}
const stores = [store("primary"), store("secondary")];
function client(
  id: string,
  auto: boolean,
  storeId = "secondary",
): AdminClientInstanceListItem {
  return {
    id,
    display_name: id,
    current_store_id: storeId,
    current_store_name: storeId,
    status: "active",
    current_credential_fingerprint: "synthetic-fingerprint",
    auto_create_wrapper_assets: auto,
    access_override: { kind: "individual" },
    actions: { can_disable: true, can_enable: false },
  };
}
const first = client("client-a", false);
const enrolled = client("client-b", true);

function harness(
  path: string,
  pages: Readonly<Record<string, ReactNode>> = {},
  state?: Record<string, unknown>,
) {
  window.history.replaceState({}, "", path);
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const view = render(
    <QueryClientProvider client={qc}>
      {withTestRouter(<SetupPage />, pages, state)}
    </QueryClientProvider>,
  );
  return { ...view, qc };
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(fetchStores).mockResolvedValue({ items: stores });
  vi.mocked(fetchClientInstances).mockResolvedValue({
    items: [first, enrolled],
  });
  vi.mocked(fetchSecretsView).mockResolvedValue({ entries: [], resources: [] });
  vi.mocked(fetchWrapperPermissionsView).mockResolvedValue({
    permissions: [],
    pairs: [],
    resources: [],
  });
  vi.mocked(postClientInstanceCreate).mockResolvedValue(enrolled);
});

describe("guided navigation ownership", () => {
  it.each(["pending", "failed"])(
    "retains requested context but disables scoped creation with %s store metadata",
    async (phase) => {
      let resolve!: (value: Awaited<ReturnType<typeof fetchStores>>) => void;
      const delayed = new Promise<Awaited<ReturnType<typeof fetchStores>>>(
        (done) => {
          resolve = done;
        },
      );
      vi.mocked(fetchStores).mockReturnValue(delayed);
      const view = harness("/setup?store=secondary#guide", {
        "/secrets": <SecretsPage />,
      });
      await screen.findByRole("heading", { name: "Get a wrapper working" });
      if (phase === "failed") {
        await act(() =>
          view.qc.cancelQueries({ queryKey: ["admin", "stores"] }),
        );
        vi.mocked(fetchStores).mockRejectedValue(
          new Error("metadata unavailable"),
        );
        await act(async () => {
          await view.qc.refetchQueries({ queryKey: ["admin", "stores"] });
        });
        await screen.findByRole("alert");
      }
      for (const name of [
        "Enroll client instance",
        "Create resource",
        "Create wrapper/resource pair",
      ]) {
        expect(screen.getByRole("button", { name })).toBeDisabled();
        expect(screen.queryByRole("link", { name })).not.toBeInTheDocument();
      }
      for (const name of ["Open Secrets", "Open wrapper permissions"]) {
        const href = screen.getByRole("link", { name }).getAttribute("href")!;
        expect(
          new URL(href, window.location.origin).searchParams.get("store"),
        ).toBe("secondary");
      }
      if (phase === "pending") {
        await userEvent
          .setup()
          .click(screen.getByRole("link", { name: "Open Secrets" }));
        await waitFor(() => expect(window.location.pathname).toBe("/secrets"));
        expect(new URLSearchParams(window.location.search).get("store")).toBe(
          "secondary",
        );
        await act(async () => resolve({ items: stores }));
        await waitFor(() =>
          expect(fetchSecretsView).toHaveBeenCalledWith("secondary"),
        );
        expect(fetchSecretsView).not.toHaveBeenCalledWith("primary");
      }
      expect(postClientInstanceCreate).not.toHaveBeenCalled();
    },
  );

  it("continues enrollment with the exact second client and retains an explicit selection through Back", async () => {
    const user = userEvent.setup();
    harness("/client-instances?store=secondary", {
      "/client-instances": (
        <CreateClientInstanceDialog
          stores={stores}
          initialStoreId="secondary"
          onClose={() => {}}
        />
      ),
      "/setup": <SetupPage />,
      "/secrets": <h2>Secrets destination</h2>,
    });
    await screen.findByRole("dialog", { name: "Create client instance" });
    fireEvent.change(screen.getByLabelText("Display name"), {
      target: { value: "client-b" },
    });
    fireEvent.change(screen.getByLabelText("Certificate PEM"), {
      target: { value: "synthetic public certificate" },
    });
    fireEvent.click(screen.getByTestId("create-auto-create"));
    await user.click(screen.getByRole("button", { name: "Create" }));
    const continuation = await screen.findByRole("link", {
      name: "Continue Get a wrapper working",
    });
    expect(
      new URL(
        continuation.getAttribute("href")!,
        window.location.origin,
      ).searchParams.get("client"),
    ).toBe(enrolled.id);
    await user.click(continuation);
    await waitFor(() =>
      expect(screen.getByLabelText("Runtime client instance")).toHaveValue(
        enrolled.id,
      ),
    );
    expect(screen.getByTestId("setup-registration-path")).toHaveTextContent(
      "Asset auto-creation is explicitly enabled for this instance.",
    );
    const review = screen.getByRole("link", {
      name: "Review client override and status",
    });
    expect(
      new URL(
        review.getAttribute("href")!,
        window.location.origin,
      ).searchParams.get("client"),
    ).toBe(enrolled.id);
    expect(postClientInstanceCreate).toHaveBeenCalledOnce();

    await user.selectOptions(
      screen.getByLabelText("Runtime client instance"),
      first.id,
    );
    await waitFor(() =>
      expect(new URLSearchParams(window.location.search).get("client")).toBe(
        first.id,
      ),
    );
    await user.click(screen.getByRole("link", { name: "Open Secrets" }));
    await screen.findByRole("heading", { name: "Secrets destination" });
    act(() => window.history.back());
    await waitFor(() =>
      expect(screen.getByLabelText("Runtime client instance")).toHaveValue(
        first.id,
      ),
    );
    expect(screen.getByTestId("setup-registration-path")).toHaveTextContent(
      "Asset auto-creation is off for this instance.",
    );
  });

  it("does not replace a requested unmapped client with another identity", async () => {
    harness("/setup?store=secondary&client=unmapped");
    await screen.findByText(/requested runtime client instance is not mapped/);
    expect(screen.getByLabelText("Runtime client instance")).toHaveValue("");
    expect(
      screen.queryByRole("link", { name: "Review client override and status" }),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("setup-registration-path")).toHaveTextContent(
      "Select or enroll a runtime client instance",
    );
  });

  it("does not present cached client settings after a metadata refresh fails", async () => {
    const view = harness("/setup?store=secondary&client=client-b");
    await waitFor(() =>
      expect(screen.getByLabelText("Runtime client instance")).toHaveValue(
        enrolled.id,
      ),
    );
    expect(screen.getByTestId("setup-registration-path")).toHaveTextContent(
      "Asset auto-creation is explicitly enabled for this instance.",
    );
    vi.mocked(fetchClientInstances).mockRejectedValue(
      new Error("client metadata unavailable"),
    );
    await act(async () => {
      await view.qc.refetchQueries({ queryKey: ["admin", "client-instances"] });
    });
    await waitFor(() =>
      expect(screen.getByLabelText("Runtime client instance")).toBeDisabled(),
    );
    expect(screen.getByLabelText("Runtime client instance")).toHaveValue("");
    expect(screen.getByTestId("setup-registration-path")).toHaveTextContent(
      "Select or enroll a runtime client instance",
    );
    expect(
      screen.queryByRole("link", { name: "Review client override and status" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(/requested runtime client instance is not mapped/),
    ).not.toBeInTheDocument();
    expect(fetchClientInstances).toHaveBeenCalledTimes(2);
  });

  it("preserves user state, search and hash when selecting the intended Setup client", async () => {
    const state = { marker: { retained: true } };
    harness("/setup?store=secondary&client=client-b&keep=yes#guide", {}, state);
    await waitFor(() =>
      expect(screen.getByLabelText("Runtime client instance")).toHaveValue(
        enrolled.id,
      ),
    );
    const length = window.history.length;
    await userEvent
      .setup()
      .selectOptions(
        screen.getByLabelText("Runtime client instance"),
        first.id,
      );
    await waitFor(() =>
      expect(new URLSearchParams(window.location.search).get("client")).toBe(
        first.id,
      ),
    );
    expect(window.history.state).toMatchObject(state);
    expect(window.history.length).toBe(length);
    expect(new URLSearchParams(window.location.search).get("keep")).toBe("yes");
    expect(new URLSearchParams(window.location.search).get("store")).toBe(
      "secondary",
    );
    expect(window.location.hash).toBe("#guide");
  });

  it("preserves user history state and length while consuming an intent and changing inventory filters", async () => {
    const state = { marker: { retained: true } };
    harness(
      "/client-instances?store=secondary&create=client&client=client-b&keep=yes#guide",
      {
        "/client-instances": <ClientInstancesPage />,
      },
      state,
    );
    const length = window.history.length;
    await screen.findByRole("dialog", { name: "Create client instance" });
    await waitFor(() =>
      expect(new URLSearchParams(window.location.search).has("create")).toBe(
        false,
      ),
    );
    expect(window.history.state).toMatchObject(state);
    expect(window.history.length).toBe(length);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await user.click(
      screen.getByRole("button", { name: "Clear client filter" }),
    );
    await waitFor(() =>
      expect(new URLSearchParams(window.location.search).has("client")).toBe(
        false,
      ),
    );
    expect(window.history.state).toMatchObject(state);
    expect(window.history.length).toBe(length);
    await user.selectOptions(
      screen.getByLabelText("Mapped store filter"),
      "secondary",
    );
    await waitFor(() =>
      expect(
        new URLSearchParams(window.location.search).get("mappedStore"),
      ).toBe("secondary"),
    );
    expect(window.history.state).toMatchObject(state);
    expect(window.history.length).toBe(length);
    expect(new URLSearchParams(window.location.search).get("keep")).toBe("yes");
    expect(window.location.hash).toBe("#guide");
  });
});
