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
  return render(
    <QueryClientProvider client={qc}>
      <DeleteResourceDialog
        storeId="prod"
        resourceId="r1"
        resourceName="github/token"
        onClose={onClose}
      />
    </QueryClientProvider>,
  );
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
        secret_count: 2,
        pair_count: 1,
        permission_count: 0,
        sample_secret_ids: [],
        sample_pairs: [],
      },
    } as never);
    renderDialog(onClose);
    await user.click(screen.getByTestId("resource-delete-confirm"));

    const blockers = await screen.findByTestId("resource-delete-blockers");
    expect(blockers).toHaveTextContent(/2 secret/i);
    expect(blockers).toHaveTextContent(/1 wrapper\/resource pair/i);
    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes after a successful delete", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    vi.mocked(deleteResource).mockResolvedValue({ ok: true } as never);
    renderDialog(onClose);
    await user.click(screen.getByTestId("resource-delete-confirm"));

    await vi.waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });
});
