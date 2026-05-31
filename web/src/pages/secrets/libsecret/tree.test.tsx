import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { LibsecretTree } from "./tree";

vi.mock("@/api/client", async () => {
  const actual =
    await vi.importActual<typeof import("@/api/client")>("@/api/client");
  return {
    ...actual,
    fetchLibsecretTree: vi.fn(),
  };
});

import { type AdminLibsecretTreeResponse, fetchLibsecretTree } from "@/api/client";

function renderTree(ui: React.ReactNode) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(<QueryClientProvider client={qc}>{ui}</QueryClientProvider>);
}

const baseTree: AdminLibsecretTreeResponse = {
  aliases: { default: "default" },
  collections: [
    {
      id: "default",
      label: "Default",
      created: 0,
      modified: 0,
      items: [
        {
          id: "gh-cli",
          label: "GitHub CLI",
          content_type: "text/plain",
          created: 0,
          modified: 0,
          attributes: { "xdg:schema": "org.freedesktop.Secret.Generic" },
        },
      ],
    },
  ],
};

describe("LibsecretTree", () => {
  beforeEach(() => {
    vi.mocked(fetchLibsecretTree).mockReset();
  });

  it("renders collections + items + aliases from the structured endpoint", async () => {
    vi.mocked(fetchLibsecretTree).mockResolvedValue(baseTree);
    renderTree(<LibsecretTree storeId="test" resourceId="github/gh" enabled />);

    const tree = await screen.findByTestId("libsecret-tree");
    expect(tree).toBeInTheDocument();

    const collection = within(tree).getByTestId(
      "libsecret-collection-default",
    );
    expect(collection).toHaveTextContent("Default");
    expect(collection).toHaveTextContent("1 item");

    const item = within(tree).getByTestId(
      "libsecret-item-default-gh-cli",
    );
    expect(item).toHaveTextContent("GitHub CLI");
    expect(item).toHaveTextContent("text/plain");
    expect(item).toHaveTextContent("xdg:schema=org.freedesktop.Secret.Generic");

    const aliases = within(tree).getByTestId("libsecret-aliases");
    expect(within(aliases).getByTestId("libsecret-alias-default")).toHaveTextContent(
      /default\s*→\s*default/,
    );
  });

  it("shows an empty-state message when the resource has no libsecret content", async () => {
    vi.mocked(fetchLibsecretTree).mockResolvedValue({
      aliases: {},
      collections: [],
    });
    renderTree(<LibsecretTree storeId="test" resourceId="empty" enabled />);
    const tree = await screen.findByTestId("libsecret-tree");
    expect(tree).toHaveTextContent(/none/i);
  });

  it("does not fetch while disabled (the parent block is collapsed)", () => {
    vi.mocked(fetchLibsecretTree).mockResolvedValue(baseTree);
    renderTree(
      <LibsecretTree storeId="test" resourceId="github/gh" enabled={false} />,
    );
    expect(fetchLibsecretTree).not.toHaveBeenCalled();
  });
});
