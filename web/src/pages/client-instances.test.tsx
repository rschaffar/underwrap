import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import {
  ClientInstancesPage,
  canManageClientInstances,
} from "./client-instances";

vi.mock("@/api/client", async () => {
  const actual = await vi.importActual<typeof import("@/api/client")>("@/api/client");
  return {
    ...actual,
    fetchClientInstances: vi.fn(),
    fetchStores: vi.fn(),
    putClientInstanceUpdate: vi.fn(),
  };
});

import {
  fetchClientInstances,
  fetchStores,
  putClientInstanceUpdate,
} from "@/api/client";

function renderWithQuery(ui: React.ReactNode) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(<QueryClientProvider client={qc}>{ui}</QueryClientProvider>);
}

describe("canManageClientInstances", () => {
  it("is admin-only", () => {
    expect(canManageClientInstances({ authenticated: true, role: "admin" })).toBe(true);
    expect(canManageClientInstances({ authenticated: true, role: "operator" })).toBe(false);
    expect(canManageClientInstances({ authenticated: false })).toBe(false);
    expect(canManageClientInstances(null)).toBe(false);
  });
});

describe("ClientInstancesPage", () => {
  const stores = {
    items: [
      {
        id: "store_work",
        name: "Work",
        source: "dynamic" as const,
        backend: "plain" as const,
        created_at: 1n,
        usage: { mapped_client_instance_count: 1, has_secrets: true },
        actions: {
          can_edit_metadata: true,
          can_rotate_key: false,
          can_delete_now: false,
          can_force_delete: false,
        },
      },
      {
        id: "store_other",
        name: "Other",
        source: "dynamic" as const,
        backend: "plain" as const,
        created_at: 2n,
        usage: { mapped_client_instance_count: 0, has_secrets: false },
        actions: {
          can_edit_metadata: true,
          can_rotate_key: false,
          can_delete_now: true,
          can_force_delete: false,
        },
      },
    ],
  };

  const instances = {
    items: [
      {
        id: "client_work",
        display_name: "Work laptop",
        hostname_hint: "deepthought",
        instance_name: "work",
        current_store_id: "store_work",
        current_store_name: "Work",
        status: "active" as const,
        last_seen_at: 1_700_000_000n,
        current_credential_fingerprint: "fp-work",
        actions: {
          can_edit_display_name: true,
          can_remap_store: true,
          can_disable: true,
          can_enable: false,
          can_delete: true,
        },
      },
      {
        id: "client_disabled",
        display_name: "Retired workstation",
        hostname_hint: "deepthought-old",
        instance_name: "legacy",
        current_store_id: "store_other",
        current_store_name: "Other",
        status: "disabled" as const,
        last_seen_at: undefined,
        current_credential_fingerprint: "fp-old",
        actions: {
          can_edit_display_name: true,
          can_remap_store: true,
          can_disable: false,
          can_enable: true,
          can_delete: true,
        },
      },
    ],
  };

  beforeEach(() => {
    vi.mocked(fetchClientInstances).mockReset();
    vi.mocked(fetchStores).mockReset();
    vi.mocked(putClientInstanceUpdate).mockReset();
    vi.mocked(fetchClientInstances).mockResolvedValue(instances);
    vi.mocked(fetchStores).mockResolvedValue(stores);
    vi.mocked(putClientInstanceUpdate).mockImplementation(async (id, body) => ({
      id,
      display_name: id === "client_work" ? "Work laptop" : "Retired workstation",
      hostname_hint: id === "client_work" ? "deepthought" : "deepthought-old",
      instance_name: id === "client_work" ? "work" : "legacy",
      current_store_id: body.current_store_id ?? (id === "client_work" ? "store_work" : "store_other"),
      current_store_name: body.current_store_id === "store_other" ? "Other" : body.current_store_id === "store_work" ? "Work" : id === "client_work" ? "Work" : "Other",
      status: body.status ?? (id === "client_work" ? "active" : "disabled"),
      last_seen_at: id === "client_work" ? 1_700_000_000n : undefined,
      current_credential_fingerprint: id === "client_work" ? "fp-work" : "fp-old",
      current_credential_issued_at: undefined,
      actions: {
        can_edit_display_name: true,
        can_remap_store: true,
        can_disable: body.status !== "disabled",
        can_enable: body.status === "disabled" || id === "client_disabled",
        can_delete: true,
      },
    }));
  });

  it("shows display name first with hostname and attested instance metadata", async () => {
    renderWithQuery(
      <ClientInstancesPage principal={{ authenticated: true, role: "admin" }} />,
    );

    const row = await screen.findByTestId("client-instance-row-client_work");
    expect(within(row).getByText("Work laptop")).toBeInTheDocument();
    expect(within(row).getByText(/deepthought · instance work/i)).toBeInTheDocument();
    expect(within(row).getByText("Work")).toBeInTheDocument();
    expect(within(row).getByText("active")).toBeInTheDocument();
  });

  it("keeps disabled instances visible and filterable", async () => {
    const user = userEvent.setup();
    renderWithQuery(
      <ClientInstancesPage principal={{ authenticated: true, role: "admin" }} />,
    );

    await screen.findByTestId("client-instance-row-client_work");
    expect(screen.getByTestId("client-instance-row-client_disabled")).toBeInTheDocument();

    await user.selectOptions(screen.getByLabelText(/status filter/i), "disabled");

    expect(screen.queryByTestId("client-instance-row-client_work")).toBeNull();
    expect(screen.getByTestId("client-instance-row-client_disabled")).toBeInTheDocument();
  });

  it("remaps an instance to another store from an explicit dialog", async () => {
    const user = userEvent.setup();
    renderWithQuery(
      <ClientInstancesPage principal={{ authenticated: true, role: "admin" }} />,
    );

    const row = await screen.findByTestId("client-instance-row-client_work");
    await user.click(within(row).getByRole("button", { name: /remap store/i }));
    const dialog = await screen.findByRole("dialog", { name: /remap store: work laptop/i });
    await user.selectOptions(within(dialog).getByLabelText(/new target store/i), "store_other");
    await user.click(within(dialog).getByRole("button", { name: /remap now/i }));

    expect(putClientInstanceUpdate).toHaveBeenCalledWith("client_work", {
      current_store_id: "store_other",
    });
  });

  it("disables an active instance via the confirmation dialog", async () => {
    const user = userEvent.setup();
    renderWithQuery(
      <ClientInstancesPage principal={{ authenticated: true, role: "admin" }} />,
    );

    const row = await screen.findByTestId("client-instance-row-client_work");
    await user.click(within(row).getByRole("button", { name: /^disable$/i }));
    const dialog = await screen.findByRole("dialog", { name: /disable: work laptop/i });
    await user.click(within(dialog).getByRole("button", { name: /disable now/i }));

    expect(putClientInstanceUpdate).toHaveBeenCalledWith("client_work", {
      status: "disabled",
    });
  });
});
