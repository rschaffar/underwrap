import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { SecretsPage, groupSecretIds } from "./secrets";

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
    postWritePermissionsGet: vi.fn(),
  };
});

import {
  fetchStores,
  postCatalogGet,
  postSecretList,
  postWritePermissionsGet,
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
    vi.mocked(postWritePermissionsGet).mockReset();
  });

  it("filters out reserved __underwrap/ secret IDs and groups the rest by resource", async () => {
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
        "github/token/env/GH_USERNAME",
        "github/gh/libsecret/credential",
        "__underwrap/policy",
        "__underwrap/catalog",
        "aws/access-key/env/AWS_KEY",
      ],
    });
    vi.mocked(postCatalogGet).mockResolvedValue({ entries: [] });
    vi.mocked(postWritePermissionsGet).mockResolvedValue({
      store: "default",
      files_writable: {},
      libsecret_writable: {},
    });

    renderWithQuery(<SecretsPage principal={null} />);

    // Resources show up as expandable blocks.
    const ghToken = await screen.findByTestId("resource-block-github/token");
    const ghGh = await screen.findByTestId("resource-block-github/gh");
    const aws = await screen.findByTestId("resource-block-aws/access-key");
    expect(ghToken).toBeInTheDocument();
    expect(ghGh).toBeInTheDocument();
    expect(aws).toBeInTheDocument();

    // The reserved internal IDs do not produce a resource block.
    expect(
      screen.queryByTestId("resource-block-__underwrap/policy"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("resource-block-__underwrap/catalog"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("resource-block-unrecognized"),
    ).not.toBeInTheDocument();

    // The github/token block's env section lists exactly the two
    // entries that belong to it.
    const ghTokenEnv = within(ghToken).getByTestId("slot-env");
    expect(within(ghTokenEnv).getByText("GH_TOKEN")).toBeInTheDocument();
    expect(within(ghTokenEnv).getByText("GH_USERNAME")).toBeInTheDocument();
    // And the libsecret + files sections of the same block are empty.
    expect(within(ghToken).getByTestId("slot-files")).toHaveTextContent(/none/i);
    expect(within(ghToken).getByTestId("slot-libsecret")).toHaveTextContent(
      /none/i,
    );

    // The github/gh block's libsecret section has the credential entry.
    const ghGhLibsecret = within(ghGh).getByTestId("slot-libsecret");
    expect(within(ghGhLibsecret).getByText("credential")).toBeInTheDocument();
  });

  it("surfaces non-conforming IDs in the unrecognized block", async () => {
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
    // No `/env|files|libsecret/` segment — these don't fit the
    // wrapper-resource layout and the page should still surface
    // them so the operator can clean up.
    vi.mocked(postSecretList).mockResolvedValue({
      ids: ["legacy-blob", "another/orphan"],
    });
    vi.mocked(postCatalogGet).mockResolvedValue({ entries: [] });
    vi.mocked(postWritePermissionsGet).mockResolvedValue({
      store: "default",
      files_writable: {},
      libsecret_writable: {},
    });

    renderWithQuery(<SecretsPage principal={null} />);

    const block = await screen.findByTestId("resource-block-unrecognized");
    expect(within(block).getByText("legacy-blob")).toBeInTheDocument();
    expect(within(block).getByText("another/orphan")).toBeInTheDocument();
  });

  it("renders the wrapper-writable toggle for files and libsecret slots but not env", async () => {
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
        "github/token/files/config.json",
        "github/gh/libsecret/credential",
      ],
    });
    vi.mocked(postCatalogGet).mockResolvedValue({ entries: [] });
    vi.mocked(postWritePermissionsGet).mockResolvedValue({
      store: "default",
      files_writable: { "github/token": true },
      libsecret_writable: {},
    });

    renderWithQuery(
      <SecretsPage
        principal={{
          authenticated: true,
          username: "alice",
          role: "admin",
        }}
      />,
    );

    // The files toggle for github/token is checked (admin set it on).
    const filesToggle = await screen.findByTestId(
      "wrapper-writable-files-github/token",
    );
    expect(filesToggle).toBeChecked();
    expect(filesToggle).toBeEnabled();
    // The libsecret toggle for github/gh is unchecked.
    const libsecretToggle = await screen.findByTestId(
      "wrapper-writable-libsecret-github/gh",
    );
    expect(libsecretToggle).not.toBeChecked();
    // env slots get no toggle — the slot has no wrapper-side write
    // path so nothing is gateable.
    expect(
      screen.queryByTestId("wrapper-writable-env-github/token"),
    ).not.toBeInTheDocument();
  });

  it("disables the wrapper-writable toggle for non-admin operators", async () => {
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
      ids: ["github/token/files/config.json"],
    });
    vi.mocked(postCatalogGet).mockResolvedValue({ entries: [] });
    vi.mocked(postWritePermissionsGet).mockResolvedValue({
      store: "default",
      files_writable: {},
      libsecret_writable: {},
    });

    renderWithQuery(
      <SecretsPage
        principal={{
          authenticated: true,
          username: "bob",
          role: "operator",
        }}
      />,
    );

    const toggle = await screen.findByTestId(
      "wrapper-writable-files-github/token",
    );
    expect(toggle).toBeDisabled();
  });
});

describe("groupSecretIds", () => {
  it("splits on the first valid slot delimiter", () => {
    const { resources, unrecognized } = groupSecretIds([
      "github/token/env/GH_TOKEN",
      "github/token/env/GH_USERNAME",
      "github/token/files/config.json",
      "github/gh/libsecret/credential",
      "aws/access-key/env/AWS_KEY",
    ]);
    expect(unrecognized).toEqual([]);
    // Resources are alphabetically sorted.
    expect(resources.map((r) => r.resource)).toEqual([
      "aws/access-key",
      "github/gh",
      "github/token",
    ]);
    const token = resources.find((r) => r.resource === "github/token")!;
    expect(token.env.map((e) => e.name)).toEqual(["GH_TOKEN", "GH_USERNAME"]);
    expect(token.files.map((e) => e.name)).toEqual(["config.json"]);
    expect(token.libsecret).toEqual([]);
  });

  it("preserves the full secret ID on every entry so dialogs can target it", () => {
    const { resources } = groupSecretIds(["github/token/env/GH_TOKEN"]);
    expect(resources[0].env[0]).toEqual({
      id: "github/token/env/GH_TOKEN",
      name: "GH_TOKEN",
    });
  });

  it("surfaces non-conforming IDs in `unrecognized` without dropping them", () => {
    const { resources, unrecognized } = groupSecretIds([
      "stray-blob",
      "no/slot/here-ish",
    ]);
    expect(resources).toEqual([]);
    expect(unrecognized).toEqual(["no/slot/here-ish", "stray-blob"]);
  });
});
