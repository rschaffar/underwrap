import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { SecretsPage } from "./secrets/page";
import { groupSecretEntries } from "./secrets/model";

// Mock the API surface; the unit tests assert rendering decisions
// only and stay hermetic from a real backend.
vi.mock("@/api/client", async () => {
  const actual =
    await vi.importActual<typeof import("@/api/client")>("@/api/client");
  return {
    ...actual,
    fetchStores: vi.fn(),
    fetchClientInstances: vi.fn(),
    fetchSecretsView: vi.fn(),
  };
});

import {
  fetchClientInstances,
  fetchSecretsView,
  fetchStores,
} from "@/api/client";

function renderWithQuery(ui: React.ReactNode) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(<QueryClientProvider client={qc}>{ui}</QueryClientProvider>);
}

describe("SecretsPage", () => {
  const storesList = {
    items: [
      {
        id: "store_default",
        name: "default",
        backend: "plain" as const,
        created_at: 0,
        locked: false,
        usage: { mapped_client_instance_count: 0, has_secrets: false },
        actions: {
          can_rotate_key: false,
          can_unlock: false,
          can_lock: false,
          can_delete_now: true,
          can_force_delete: false,
        },
      },
    ],
  };

  beforeEach(() => {
    vi.mocked(fetchStores).mockReset();
    vi.mocked(fetchClientInstances).mockReset();
    vi.mocked(fetchSecretsView).mockReset();
    vi.mocked(fetchSecretsView).mockResolvedValue({
      entries: [],
      resources: [],
    });
    vi.mocked(fetchClientInstances).mockResolvedValue({ items: [] });
  });

  it("groups structured secret entries by resource", async () => {
    vi.mocked(fetchStores).mockResolvedValue(storesList);
    vi.mocked(fetchSecretsView).mockResolvedValue({
      entries: [
        { resource_id: "github/token", slot: "env", key: "GH_TOKEN" },
        { resource_id: "github/token", slot: "env", key: "GH_USERNAME" },
        { resource_id: "github/gh", slot: "libsecret", key: "credential" },
        { resource_id: "aws/access-key", slot: "env", key: "AWS_KEY" },
      ],
      resources: [],
    });
    renderWithQuery(<SecretsPage />);

    const ghToken = await screen.findByTestId("resource-block-github/token");
    const ghGh = await screen.findByTestId("resource-block-github/gh");
    const aws = await screen.findByTestId("resource-block-aws/access-key");
    expect(ghToken).toBeInTheDocument();
    expect(ghGh).toBeInTheDocument();
    expect(aws).toBeInTheDocument();

    // The github/token block's env section lists exactly the two
    // entries that belong to it.
    const ghTokenEnv = within(ghToken).getByTestId("slot-env");
    expect(within(ghTokenEnv).getByText("GH_TOKEN")).toBeInTheDocument();
    expect(within(ghTokenEnv).getByText("GH_USERNAME")).toBeInTheDocument();
    expect(within(ghToken).getByTestId("slot-files")).toHaveTextContent(/none/i);
    expect(within(ghToken).getByTestId("slot-libsecret")).toHaveTextContent(
      /none/i,
    );

    const ghGhLibsecret = within(ghGh).getByTestId("slot-libsecret");
    expect(within(ghGhLibsecret).getByText("credential")).toBeInTheDocument();
  });

  it("lists resources from the backend even when they hold zero secrets", async () => {
    // Critical for the seed flow: an admin sees a resource block as
    // soon as a resource record exists, so they can write the first
    // entries before the wrapper's first read.
    vi.mocked(fetchStores).mockResolvedValue(storesList);
    vi.mocked(fetchSecretsView).mockResolvedValue({
      entries: [],
      resources: [
        {
          id: "res_gh",
          name: "github/gh",
          created_by: { kind: "operator", id: "op-1", username: "alice" },
          created_at: 1_700_000_000,
        },
      ],
    });

    renderWithQuery(<SecretsPage />);

    const block = await screen.findByTestId("resource-block-res_gh");
    expect(block).toBeInTheDocument();
    expect(within(block).getByTestId("slot-libsecret")).toHaveTextContent(
      /none/i,
    );
    expect(
      within(block).getByTestId("resource-provenance-res_gh"),
    ).toHaveTextContent(/by alice/i);
  });

  it("shows client-instance display name in resource provenance", async () => {
    vi.mocked(fetchStores).mockResolvedValue(storesList);
    vi.mocked(fetchSecretsView).mockResolvedValue({
      entries: [],
      resources: [
        {
          id: "res_auto",
          name: "auto/created",
          created_by: { kind: "client-instance", id: "inst_abc123" },
          created_at: 1_700_000_000,
        },
      ],
    });
    vi.mocked(fetchClientInstances).mockResolvedValue({
      items: [
        {
          id: "inst_abc123",
          display_name: "shame01",
          current_store_id: "store_default",
          current_store_name: "default",
          status: "active",
          current_credential_fingerprint: "fp",
          auto_create_wrapper_assets: true,
          actions: {
            can_disable: true,
            can_enable: false,
          },
        },
      ],
    });

    renderWithQuery(<SecretsPage />);

    const provenance = await screen.findByTestId(
      "resource-provenance-res_auto",
    );
    expect(provenance.textContent).toMatch(/shame01/);
    expect(provenance.textContent).not.toMatch(/inst_abc123/);
  });
});

describe("groupSecretEntries", () => {
  it("buckets by resource and sorts each slot by key", () => {
    const groups = groupSecretEntries([
      { resource_id: "github/token", slot: "env", key: "GH_USERNAME" },
      { resource_id: "github/token", slot: "env", key: "GH_TOKEN" },
      { resource_id: "github/token", slot: "files", key: "config.json" },
      { resource_id: "github/gh", slot: "libsecret", key: "credential" },
      { resource_id: "aws/access-key", slot: "env", key: "AWS_KEY" },
    ]);
    expect(groups.map((r) => r.resource_id)).toEqual([
      "aws/access-key",
      "github/gh",
      "github/token",
    ]);
    const token = groups.find((r) => r.resource_id === "github/token")!;
    expect(token.env.map((e) => e.key)).toEqual(["GH_TOKEN", "GH_USERNAME"]);
    expect(token.files.map((e) => e.key)).toEqual(["config.json"]);
    expect(token.libsecret).toEqual([]);
  });
});
