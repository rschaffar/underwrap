import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import {
  deleteClientInstance,
  fetchClientInstances,
  fetchStores,
  postClientInstanceCreate,
  putClientInstanceUpdate,
} from "@/api/client";

import { ClientInstancesPage } from "./client-instances/page";
import { withTestRouter } from "@/test-router";

vi.mock("@/api/client", async () => {
  const actual =
    await vi.importActual<typeof import("@/api/client")>("@/api/client");
  return {
    ...actual,
    fetchClientInstances: vi.fn(),
    fetchStores: vi.fn(),
    putClientInstanceUpdate: vi.fn(),
    postClientInstanceCreate: vi.fn(),
    deleteClientInstance: vi.fn(),
  };
});

function renderClientInstancesPage() {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={qc}>
      {withTestRouter(<ClientInstancesPage />)}
    </QueryClientProvider>,
  );
}

async function openWorkEditDialog(user: ReturnType<typeof userEvent.setup>) {
  renderClientInstancesPage();
  const row = await screen.findByTestId("client-instance-row-client_work");
  await user.click(within(row).getByRole("button", { name: "Edit" }));
  return screen.findByRole("dialog", { name: "Edit: Work laptop" });
}

function expectTextInOrder(element: HTMLElement, fragments: string[]) {
  const text = element.textContent ?? "";
  let cursor = -1;
  for (const fragment of fragments) {
    const next = text.indexOf(fragment, cursor + 1);
    expect(
      next,
      `expected ${JSON.stringify(fragment)} after offset ${cursor}`,
    ).toBeGreaterThan(cursor);
    cursor = next;
  }
}

describe("ClientInstancesPage", () => {
  const stores = {
    items: [
      {
        id: "store_work",
        name: "Work",
        backend: "plain" as const,
        created_at: 1,
        lifecycle_state: "ready" as const,
        usage: { mapped_client_instance_count: 1, has_secrets: true },
        actions: {
          can_update: true,
          can_migrate: true,
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
  };

  const instances = {
    items: [
      {
        id: "client_work",
        display_name: "Work laptop",
        current_store_id: "store_work",
        current_store_name: "Work",
        status: "active" as const,
        last_seen_at: 1_700_000_000,
        access_override: { kind: "individual" as const },
        current_credential_fingerprint: "fp-work",
        auto_create_wrapper_assets: false,
        actions: {
          can_disable: true,
          can_enable: false,
        },
      },
      {
        id: "client_disabled",
        display_name: "Retired workstation",
        current_store_id: "store_other",
        current_store_name: "Other",
        status: "disabled" as const,
        last_seen_at: undefined,
        access_override: { kind: "individual" as const },
        current_credential_fingerprint: "fp-old",
        auto_create_wrapper_assets: false,
        actions: {
          can_disable: false,
          can_enable: true,
        },
      },
    ],
  };

  beforeEach(() => {
    vi.mocked(fetchClientInstances).mockReset();
    vi.mocked(fetchStores).mockReset();
    vi.mocked(putClientInstanceUpdate).mockReset();
    vi.mocked(postClientInstanceCreate).mockReset();
    vi.mocked(deleteClientInstance).mockReset();
    vi.mocked(deleteClientInstance).mockResolvedValue(undefined as never);
    vi.mocked(fetchClientInstances).mockResolvedValue(instances);
    vi.mocked(fetchStores).mockResolvedValue(stores);
    vi.mocked(postClientInstanceCreate).mockResolvedValue({
      id: "client_new",
      display_name: "new",
      current_store_id: "store_work",
      current_store_name: "Work",
      status: "active",
      last_seen_at: undefined,
      access_override: { kind: "individual" as const },
      current_credential_fingerprint: "fp-new",
      auto_create_wrapper_assets: false,
      actions: {
        can_disable: true,
        can_enable: false,
      },
    });
    vi.mocked(putClientInstanceUpdate).mockImplementation(async (id, body) => ({
      id,
      display_name:
        body.display_name ??
        (id === "client_work" ? "Work laptop" : "Retired workstation"),
      current_store_id:
        body.current_store_id ??
        (id === "client_work" ? "store_work" : "store_other"),
      current_store_name:
        body.current_store_id === "store_other"
          ? "Other"
          : body.current_store_id === "store_work"
            ? "Work"
            : id === "client_work"
              ? "Work"
              : "Other",
      status: body.status ?? (id === "client_work" ? "active" : "disabled"),
      last_seen_at: id === "client_work" ? 1_700_000_000 : undefined,
      access_override: { kind: "individual" as const },
      current_credential_fingerprint:
        id === "client_work" ? "fp-work" : "fp-old",
      auto_create_wrapper_assets: false,
      actions: {
        can_disable: body.status !== "disabled",
        can_enable: body.status === "disabled" || id === "client_disabled",
      },
    }));
  });

  it("disables pristine Save and handles a programmatic no-op without a request or network error", async () => {
    const user = userEvent.setup();
    const dialog = await openWorkEditDialog(user);
    const save = within(dialog).getByRole("button", { name: "Save" });
    expect(save).toBeDisabled();
    const name = within(dialog).getByLabelText("Display name");
    await user.type(name, " amended");
    expect(save).toBeEnabled();
    await user.clear(name);
    await user.type(name, "Work laptop");
    expect(save).toBeDisabled();
    fireEvent.submit(dialog.querySelector("form")!);
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "Edit: Work laptop" }),
      ).not.toBeInTheDocument(),
    );
    expect(putClientInstanceUpdate).not.toHaveBeenCalled();
    expect(screen.queryByText(/network error/i)).not.toBeInTheDocument();
  });

  it("shows display name, store, and status for each instance", async () => {
    renderClientInstancesPage();

    const row = await screen.findByTestId("client-instance-row-client_work");
    expect(within(row).getByText("Work laptop")).toBeInTheDocument();
    expect(within(row).getByText("Work")).toBeInTheDocument();
    expect(within(row).getByText("active")).toBeInTheDocument();
    expect(
      screen.queryByText(/edits apply on the next runtime request/i),
    ).not.toBeInTheDocument();
  });

  it("keeps disabled instances visible and filterable", async () => {
    const user = userEvent.setup();
    renderClientInstancesPage();

    await screen.findByTestId("client-instance-row-client_work");
    expect(
      screen.getByTestId("client-instance-row-client_disabled"),
    ).toBeInTheDocument();

    await user.selectOptions(
      screen.getByLabelText(/status filter/i),
      "disabled",
    );

    expect(screen.queryByTestId("client-instance-row-client_work")).toBeNull();
    expect(
      screen.getByTestId("client-instance-row-client_disabled"),
    ).toBeInTheDocument();
  });

  it("remaps an instance to another store from the unified edit dialog", async () => {
    const user = userEvent.setup();
    const dialog = await openWorkEditDialog(user);
    await user.selectOptions(
      within(dialog).getByLabelText(/target store/i),
      "store_other",
    );
    await user.click(within(dialog).getByRole("button", { name: /^save$/i }));

    expect(putClientInstanceUpdate).toHaveBeenCalledWith("client_work", {
      current_store_id: "store_other",
    });
  });

  it("disables an active instance via the confirmation dialog", async () => {
    const user = userEvent.setup();
    renderClientInstancesPage();

    const row = await screen.findByTestId("client-instance-row-client_work");
    await user.click(within(row).getByRole("button", { name: /^disable$/i }));
    const dialog = await screen.findByRole("dialog", {
      name: /disable: work laptop/i,
    });
    await user.click(
      within(dialog).getByRole("button", { name: /disable now/i }),
    );

    expect(putClientInstanceUpdate).toHaveBeenCalledWith("client_work", {
      status: "disabled",
    });
  });

  it("creates a client instance via the create dialog using a pasted cert PEM", async () => {
    const user = userEvent.setup();
    renderClientInstancesPage();

    await screen.findByTestId("client-instance-row-client_work");
    await user.click(
      screen.getByRole("button", { name: /create client instance/i }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: /create client instance/i,
    });
    await user.type(
      within(dialog).getByLabelText(/display name/i),
      "laptop-work",
    );
    const pem =
      "-----BEGIN CERTIFICATE-----\nFAKECERT\n-----END CERTIFICATE-----\n";
    await user.type(within(dialog).getByLabelText(/certificate pem/i), pem);
    await user.click(within(dialog).getByRole("button", { name: /^create$/i }));

    expect(postClientInstanceCreate).toHaveBeenCalledWith({
      display_name: "laptop-work",
      current_store_id: "store_work",
      status: "active",
      cert_pem: pem.trim(),
      auto_create_wrapper_assets: false,
    });
  });

  it("renames an instance without telling metadata-only edits to restart the daemon", async () => {
    const user = userEvent.setup();
    const dialog = await openWorkEditDialog(user);
    expect(dialog).toHaveTextContent(
      "If you replace the certificate, complete the restart operation described beside that field.",
    );
    expect(dialog).not.toHaveTextContent(
      /(?:rename|remap|metadata).*requires? (?:a )?(?:daemon )?restart/i,
    );
    const input = within(dialog).getByLabelText(/display name/i);
    await user.clear(input);
    await user.type(input, "Work laptop renamed");
    await user.click(within(dialog).getByRole("button", { name: /^save$/i }));

    expect(putClientInstanceUpdate).toHaveBeenCalledWith("client_work", {
      display_name: "Work laptop renamed",
    });
  });

  it("replaces only the registered cert and pins the ordered restart operation", async () => {
    const user = userEvent.setup();
    const dialog = await openWorkEditDialog(user);
    expectTextInOrder(dialog, [
      "underwrap-keygen --instance-config /etc/underwrap/daemon/<instance>.yaml",
      "Paste the public certificate here and select Save on this same client instance.",
      "systemctl restart underwrapd-<instance>.service",
      "Verify a representative wrapper.",
    ]);
    expect(dialog).toHaveTextContent(
      "The running daemon continues using its cached old identity until restart. After Save and before restart, expect an interruption: wrapper requests fail with enrollment guidance.",
    );

    const pem =
      "-----BEGIN CERTIFICATE-----\nFRESHCERT\n-----END CERTIFICATE-----\n";
    await user.type(within(dialog).getByLabelText(/new certificate pem/i), pem);
    await user.click(within(dialog).getByRole("button", { name: /^save$/i }));

    expect(putClientInstanceUpdate).toHaveBeenCalledWith("client_work", {
      cert_pem: pem.trim(),
    });
  });

  it("toggles auto-create wrapper assets via the edit dialog", async () => {
    const user = userEvent.setup();
    const dialog = await openWorkEditDialog(user);
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

  it("surfaces a stores-load failure and disables the Create button", async () => {
    vi.mocked(fetchStores).mockRejectedValue(new Error("boom"));
    renderClientInstancesPage();

    // The instances themselves still render; only store-dependent actions
    // are blocked.
    await screen.findByTestId("client-instance-row-client_work");
    expect(
      screen.getByText(/store list could not be loaded/i),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /create client instance/i }),
    ).toBeDisabled();
  });

  it("deletes a client instance via the confirmation dialog", async () => {
    const user = userEvent.setup();
    renderClientInstancesPage();

    const row = await screen.findByTestId("client-instance-row-client_work");
    await user.click(within(row).getByRole("button", { name: /^delete$/i }));
    const dialog = await screen.findByRole("dialog", {
      name: /delete client instance: work laptop/i,
    });
    await user.click(within(dialog).getByRole("button", { name: /^delete$/i }));

    expect(deleteClientInstance).toHaveBeenCalledWith("client_work");
  });

  it("re-enables a disabled instance via the confirmation dialog", async () => {
    const user = userEvent.setup();
    renderClientInstancesPage();

    const row = await screen.findByTestId(
      "client-instance-row-client_disabled",
    );
    await user.click(within(row).getByRole("button", { name: /re-enable/i }));
    const dialog = await screen.findByRole("dialog", {
      name: /re-enable: retired workstation/i,
    });
    await user.click(
      within(dialog).getByRole("button", { name: /re-enable/i }),
    );

    expect(putClientInstanceUpdate).toHaveBeenCalledWith("client_disabled", {
      status: "active",
    });
  });
});
