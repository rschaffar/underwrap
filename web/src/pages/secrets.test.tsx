import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { SecretsPage } from "./secrets";

// Mock the API surface; the unit tests assert rendering decisions
// only and stay hermetic from a real backend.
vi.mock("@/api/client", async () => {
  const actual =
    await vi.importActual<typeof import("@/api/client")>("@/api/client");
  return {
    ...actual,
    fetchStores: vi.fn(),
    postSecretList: vi.fn(),
    postCatalogGet: vi.fn(),
    postWritesEnabledGet: vi.fn(),
  };
});

import {
  fetchStores,
  postCatalogGet,
  postSecretList,
  postWritesEnabledGet,
} from "@/api/client";

function renderWithQuery(ui: React.ReactNode) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(<QueryClientProvider client={qc}>{ui}</QueryClientProvider>);
}

describe("SecretsPage", () => {
  beforeEach(() => {
    vi.mocked(fetchStores).mockReset();
    vi.mocked(postSecretList).mockReset();
    vi.mocked(postCatalogGet).mockReset();
    vi.mocked(postWritesEnabledGet).mockReset();
  });

  it("filters out reserved __underwrap/ secret IDs from the rendered list", async () => {
    vi.mocked(fetchStores).mockResolvedValue({
      items: [
        {
          name: "default",
          source: "dynamic",
          backend: "plain",
          created_at: 0n,
        },
      ],
    });
    vi.mocked(postSecretList).mockResolvedValue({
      ids: [
        "github/token/env/GH_TOKEN",
        "__underwrap/policy",
        "__underwrap/catalog",
        "aws/access-key/env/AWS_KEY",
      ],
    });
    vi.mocked(postCatalogGet).mockResolvedValue({ entries: [] });
    vi.mocked(postWritesEnabledGet).mockResolvedValue({
      store: "default",
      enabled: true,
    });

    renderWithQuery(<SecretsPage principal={null} />);

    // Real secrets are listed.
    expect(
      await screen.findByRole("cell", { name: "github/token/env/GH_TOKEN" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("cell", { name: "aws/access-key/env/AWS_KEY" }),
    ).toBeInTheDocument();
    // The reserved internal IDs are filtered out — they exist in the
    // backend but the UI hides them so operators don't foot-gun by
    // editing or revealing the policy document through this page.
    expect(
      screen.queryByRole("cell", { name: "__underwrap/policy" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("cell", { name: "__underwrap/catalog" }),
    ).not.toBeInTheDocument();
  });

  it("disables the Create / Edit / Delete buttons when writes_enabled is false", async () => {
    vi.mocked(fetchStores).mockResolvedValue({
      items: [
        {
          name: "default",
          source: "dynamic",
          backend: "plain",
          created_at: 0n,
        },
      ],
    });
    vi.mocked(postSecretList).mockResolvedValue({
      ids: ["github/token/env/GH_TOKEN"],
    });
    vi.mocked(postCatalogGet).mockResolvedValue({ entries: [] });
    vi.mocked(postWritesEnabledGet).mockResolvedValue({
      store: "default",
      enabled: false,
    });

    renderWithQuery(<SecretsPage principal={null} />);

    // Wait for the row to land, then assert on per-row button state.
    await screen.findByRole("cell", { name: "github/token/env/GH_TOKEN" });
    // Create-secret button on the header is disabled.
    expect(
      screen.getByRole("button", { name: /create secret/i }),
    ).toBeDisabled();
    // Reveal stays enabled (read-only path).
    expect(screen.getByRole("button", { name: /reveal/i })).toBeEnabled();
    // Edit and Delete on the row are disabled.
    expect(screen.getByRole("button", { name: /^edit$/i })).toBeDisabled();
    expect(screen.getByRole("button", { name: /^delete$/i })).toBeDisabled();
    // And a "writes are currently disabled" status banner is up.
    expect(screen.getByRole("status")).toHaveTextContent(
      /writes are currently disabled/i,
    );
  });
});
