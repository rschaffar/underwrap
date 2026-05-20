import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { ClientInstancesPage } from "./client-instances";

vi.mock("@/api/client", async () => {
  const actual = await vi.importActual<typeof import("@/api/client")>("@/api/client");
  return {
    ...actual,
    fetchClientInstances: vi.fn(),
    fetchStores: vi.fn(),
    putClientInstanceUpdate: vi.fn(),
    postClientInstanceCreate: vi.fn(),
  };
});

import {
  fetchClientInstances,
  fetchStores,
  postClientInstanceCreate,
  putClientInstanceUpdate,
} from "@/api/client";

function renderWithQuery(ui: React.ReactNode) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(<QueryClientProvider client={qc}>{ui}</QueryClientProvider>);
}

describe("ClientInstancesPage", () => {
  const stores = {
    items: [
      {
        id: "store_work",
        name: "Work",
        backend: "plain" as const,
        created_at: 1n,
        locked: false,
        usage: { mapped_client_instance_count: 1, has_secrets: true },
        actions: {
          can_edit_metadata: true,
          can_rotate_key: false,
          can_unlock: false,
          can_lock: false,
          can_delete_now: false,
          can_force_delete: false,
        },
      },
      {
        id: "store_other",
        name: "Other",
        backend: "plain" as const,
        created_at: 2n,
        locked: false,
        usage: { mapped_client_instance_count: 0, has_secrets: false },
        actions: {
          can_edit_metadata: true,
          can_rotate_key: false,
          can_unlock: false,
          can_lock: false,
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
        current_store_id: "store_work",
        current_store_name: "Work",
        status: "active" as const,
        last_seen_at: 1_700_000_000n,
        current_credential_fingerprint: "fp-work",
        auto_create_wrapper_assets: false,
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
        current_store_id: "store_other",
        current_store_name: "Other",
        status: "disabled" as const,
        last_seen_at: undefined,
        current_credential_fingerprint: "fp-old",
        auto_create_wrapper_assets: false,
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
    vi.mocked(postClientInstanceCreate).mockReset();
    vi.mocked(fetchClientInstances).mockResolvedValue(instances);
    vi.mocked(fetchStores).mockResolvedValue(stores);
    vi.mocked(postClientInstanceCreate).mockResolvedValue({
      id: "client_new",
      display_name: "new",
      current_store_id: "store_work",
      current_store_name: "Work",
      status: "active",
      last_seen_at: undefined,
      current_credential_fingerprint: "fp-new",
      current_credential_issued_at: undefined,
      auto_create_wrapper_assets: false,
      actions: {
        can_edit_display_name: true,
        can_remap_store: true,
        can_disable: true,
        can_enable: false,
        can_delete: true,
      },
    });
    vi.mocked(putClientInstanceUpdate).mockImplementation(async (id, body) => ({
      id,
      display_name:
        body.display_name ??
        (id === "client_work" ? "Work laptop" : "Retired workstation"),
      current_store_id: body.current_store_id ?? (id === "client_work" ? "store_work" : "store_other"),
      current_store_name: body.current_store_id === "store_other" ? "Other" : body.current_store_id === "store_work" ? "Work" : id === "client_work" ? "Work" : "Other",
      status: body.status ?? (id === "client_work" ? "active" : "disabled"),
      last_seen_at: id === "client_work" ? 1_700_000_000n : undefined,
      current_credential_fingerprint: id === "client_work" ? "fp-work" : "fp-old",
      current_credential_issued_at: undefined,
      auto_create_wrapper_assets: false,
      actions: {
        can_edit_display_name: true,
        can_remap_store: true,
        can_disable: body.status !== "disabled",
        can_enable: body.status === "disabled" || id === "client_disabled",
        can_delete: true,
      },
    }));
  });

  it("shows display name, store, and status for each instance", async () => {
    renderWithQuery(
      <ClientInstancesPage principal={{ authenticated: true }} />,
    );

    const row = await screen.findByTestId("client-instance-row-client_work");
    expect(within(row).getByText("Work laptop")).toBeInTheDocument();
    expect(within(row).getByText("Work")).toBeInTheDocument();
    expect(within(row).getByText("active")).toBeInTheDocument();
  });

  it("keeps disabled instances visible and filterable", async () => {
    const user = userEvent.setup();
    renderWithQuery(
      <ClientInstancesPage principal={{ authenticated: true }} />,
    );

    await screen.findByTestId("client-instance-row-client_work");
    expect(screen.getByTestId("client-instance-row-client_disabled")).toBeInTheDocument();

    await user.selectOptions(screen.getByLabelText(/status filter/i), "disabled");

    expect(screen.queryByTestId("client-instance-row-client_work")).toBeNull();
    expect(screen.getByTestId("client-instance-row-client_disabled")).toBeInTheDocument();
  });

  it("remaps an instance to another store from the unified edit dialog", async () => {
    const user = userEvent.setup();
    renderWithQuery(
      <ClientInstancesPage principal={{ authenticated: true }} />,
    );

    const row = await screen.findByTestId("client-instance-row-client_work");
    await user.click(within(row).getByRole("button", { name: /^edit$/i }));
    const dialog = await screen.findByRole("dialog", { name: /edit: work laptop/i });
    await user.selectOptions(within(dialog).getByLabelText(/target store/i), "store_other");
    await user.click(within(dialog).getByRole("button", { name: /^save$/i }));

    expect(putClientInstanceUpdate).toHaveBeenCalledWith("client_work", {
      current_store_id: "store_other",
    });
  });

  it("disables an active instance via the confirmation dialog", async () => {
    const user = userEvent.setup();
    renderWithQuery(
      <ClientInstancesPage principal={{ authenticated: true }} />,
    );

    const row = await screen.findByTestId("client-instance-row-client_work");
    await user.click(within(row).getByRole("button", { name: /^disable$/i }));
    const dialog = await screen.findByRole("dialog", { name: /disable: work laptop/i });
    await user.click(within(dialog).getByRole("button", { name: /disable now/i }));

    expect(putClientInstanceUpdate).toHaveBeenCalledWith("client_work", {
      status: "disabled",
    });
  });

  it("creates a client instance via the create dialog using a pasted cert PEM", async () => {
    const user = userEvent.setup();
    renderWithQuery(
      <ClientInstancesPage principal={{ authenticated: true }} />,
    );

    await screen.findByTestId("client-instance-row-client_work");
    await user.click(screen.getByRole("button", { name: /create client instance/i }));
    const dialog = await screen.findByRole("dialog", {
      name: /create client instance/i,
    });
    await user.type(within(dialog).getByLabelText(/display name/i), "laptop-work");
    const pem = "-----BEGIN CERTIFICATE-----\nFAKECERT\n-----END CERTIFICATE-----\n";
    await user.type(within(dialog).getByLabelText(/certificate pem/i), pem);
    await user.click(within(dialog).getByRole("button", { name: /^create$/i }));

    expect(postClientInstanceCreate).toHaveBeenCalledWith({
      display_name: "laptop-work",
      current_store_id: "store_work",
      status: "active",
      cert_pem: pem.trim(),
    });
  });

  it("renames an instance via the unified edit dialog", async () => {
    const user = userEvent.setup();
    renderWithQuery(
      <ClientInstancesPage principal={{ authenticated: true }} />,
    );

    const row = await screen.findByTestId("client-instance-row-client_work");
    await user.click(within(row).getByRole("button", { name: /^edit$/i }));
    const dialog = await screen.findByRole("dialog", { name: /edit: work laptop/i });
    const input = within(dialog).getByLabelText(/display name/i);
    await user.clear(input);
    await user.type(input, "Work laptop renamed");
    await user.click(within(dialog).getByRole("button", { name: /^save$/i }));

    expect(putClientInstanceUpdate).toHaveBeenCalledWith("client_work", {
      display_name: "Work laptop renamed",
    });
  });

  it("replaces the registered cert via the unified edit dialog", async () => {
    const user = userEvent.setup();
    renderWithQuery(
      <ClientInstancesPage principal={{ authenticated: true }} />,
    );

    const row = await screen.findByTestId("client-instance-row-client_work");
    await user.click(within(row).getByRole("button", { name: /^edit$/i }));
    const dialog = await screen.findByRole("dialog", { name: /edit: work laptop/i });
    const pem = "-----BEGIN CERTIFICATE-----\nFRESHCERT\n-----END CERTIFICATE-----\n";
    await user.type(within(dialog).getByLabelText(/new certificate pem/i), pem);
    await user.click(within(dialog).getByRole("button", { name: /^save$/i }));

    expect(putClientInstanceUpdate).toHaveBeenCalledWith("client_work", {
      cert_pem: pem.trim(),
    });
  });

  it("toggles auto-create wrapper assets via the edit dialog", async () => {
    const user = userEvent.setup();
    renderWithQuery(
      <ClientInstancesPage principal={{ authenticated: true }} />,
    );

    const row = await screen.findByTestId("client-instance-row-client_work");
    await user.click(within(row).getByRole("button", { name: /^edit$/i }));
    const dialog = await screen.findByRole("dialog", { name: /edit: work laptop/i });
    // Default fixture has the flag off; the checkbox starts unchecked.
    const checkbox = within(dialog).getByTestId("edit-auto-create");
    expect(checkbox).not.toBeChecked();
    await user.click(checkbox);
    expect(checkbox).toBeChecked();
    await user.click(within(dialog).getByRole("button", { name: /^save$/i }));

    expect(putClientInstanceUpdate).toHaveBeenCalledWith("client_work", {
      auto_create_wrapper_assets: true,
    });
  });
});
