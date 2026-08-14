import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { deleteResource } from "@/api/client";

import { DeleteResourceDialog } from "./delete-resource";

vi.mock("@/api/client", async () => {
  const actual = await vi.importActual<typeof import("@/api/client")>(
    "@/api/client",
  );
  return { ...actual, deleteResource: vi.fn() };
});

function renderDialog(onClose = () => {}) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const rendered = render(
    <QueryClientProvider client={qc}>
      <DeleteResourceDialog
        storeId="prod"
        resourceId="r1"
        resourceName="github/token"
        onClose={onClose}
      />
    </QueryClientProvider>,
  );
  return { ...rendered, qc };
}

describe("DeleteResourceDialog", () => {
  beforeEach(() => {
    vi.mocked(deleteResource).mockReset();
  });

  it("renders the blockers alert and keeps the dialog open on a soft refusal", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    vi.mocked(deleteResource).mockResolvedValue({
      ok: false,
      blocked_by: {
        content_entry_count: 5,
        pair_count: 1,
        permission_count: 1,
        sample_content: [
          { kind: "env", name: "TOKEN<script>" },
          { kind: "file", path: "config/<img src=x>" },
          { kind: "libsecret_alias", name: "default&alias" },
          { kind: "libsecret_collection", collection_id: "collection<id>" },
          {
            kind: "libsecret_item",
            collection_id: "collection<id>",
            item_id: "item&one",
          },
        ],
        sample_pairs: [{ wrapper: "gh<cli>", resource: "github/token" }],
        sample_permissions: [
          {
            client_instance: "instance&one",
            wrapper: "gh<cli>",
            resource_id: "r1",
          },
        ],
      },
    } as never);
    const { qc, container } = renderDialog(onClose);
    const invalidate = vi.spyOn(qc, "invalidateQueries");
    await user.click(screen.getByTestId("resource-delete-confirm"));

    const blockers = await screen.findByTestId("resource-delete-blockers");
    expect(blockers).toHaveTextContent(/5 stored resource content entries/i);
    expect(blockers).toHaveTextContent("env: TOKEN<script>");
    expect(blockers).toHaveTextContent("file: config/<img src=x>");
    expect(blockers).toHaveTextContent("libsecret alias: default&alias");
    expect(blockers).toHaveTextContent("libsecret collection: collection<id>");
    expect(blockers).toHaveTextContent(
      "libsecret item: collection<id>/item&one",
    );
    expect(blockers).toHaveTextContent("gh<cli>/github/token");
    expect(blockers).toHaveTextContent("instance&one/gh<cli>/r1");
    expect(blockers).toHaveTextContent(/1 permission row/i);
    expect(container.querySelector("script")).toBeNull();
    expect(container.querySelector("img")).toBeNull();
    expect(invalidate).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes after a successful delete", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    vi.mocked(deleteResource).mockResolvedValue({ ok: true } as never);
    const { qc } = renderDialog(onClose);
    const invalidate = vi.spyOn(qc, "invalidateQueries");
    await user.click(screen.getByTestId("resource-delete-confirm"));

    await vi.waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(invalidate).toHaveBeenCalledTimes(2);
  });
});
