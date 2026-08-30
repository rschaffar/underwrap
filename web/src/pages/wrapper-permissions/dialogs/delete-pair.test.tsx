import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  ApiError,
  deletePair,
  type WrapperResourcePairRecord,
} from "@/api/client";

import { DeletePairDialog } from "./delete-pair";

vi.mock("@/api/client", async () => {
  const actual =
    await vi.importActual<typeof import("@/api/client")>("@/api/client");
  return { ...actual, deletePair: vi.fn() };
});

const pair: WrapperResourcePairRecord = {
  wrapper: "gh-cli",
  resource_id: "res-one",
  created_by: { kind: "operator", id: "operator-one", username: "alice" },
  created_at: 100,
  last_used_at: null,
};

function renderDialog(onClose = () => {}) {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
    },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <DeletePairDialog
        storeId="prod"
        pair={pair}
        displayNameById={new Map([["client-one", "Production client"]])}
        resourceName="github/token"
        onClose={onClose}
      />
    </QueryClientProvider>,
  );
  return queryClient;
}

describe("DeletePairDialog", () => {
  beforeEach(() => {
    vi.mocked(deletePair).mockReset();
  });

  it("renders blockers without invalidating or closing after a soft refusal", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    vi.mocked(deletePair).mockResolvedValue({
      ok: false,
      blocked_by: {
        permission_count: 1,
        sample_permissions: [
          {
            client_instance: "client-one",
            wrapper: "gh-cli",
            resource_id: "res-one",
          },
        ],
      },
    } as never);
    const queryClient = renderDialog(onClose);
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const dialog = screen.getByRole("dialog", { name: /delete pair/i });

    await user.click(within(dialog).getByRole("button", { name: "Delete" }));

    expect(
      await within(dialog).findByText(/1 wrapper permission still reference/i),
    ).toBeVisible();
    expect(within(dialog).getByText("Production client")).toBeVisible();
    expect(invalidate).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("invalidates exactly once and closes after success", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    vi.mocked(deletePair).mockResolvedValue({ ok: true } as never);
    const queryClient = renderDialog(onClose);
    const invalidate = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockResolvedValue();
    const dialog = screen.getByRole("dialog", { name: /delete pair/i });

    await user.click(within(dialog).getByRole("button", { name: "Delete" }));

    await vi.waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["admin", "wrapper-permissions-view", "prod"],
    });
  });

  it("keeps the dialog retryable without invalidating after rejection", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    vi.mocked(deletePair).mockRejectedValue(
      new ApiError(503, "synthetic_failure", "Synthetic pair failure."),
    );
    const queryClient = renderDialog(onClose);
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const dialog = screen.getByRole("dialog", { name: /delete pair/i });
    const deleteButton = within(dialog).getByRole("button", { name: "Delete" });

    await user.click(deleteButton);

    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "Synthetic pair failure.",
    );
    expect(dialog).toBeVisible();
    expect(deleteButton).toBeEnabled();
    expect(invalidate).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });
});
