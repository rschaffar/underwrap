import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { putWrapperPermission } from "@/api/client";

import { EditCellDialog } from "./edit-cell";

vi.mock("@/api/client", async () => {
  const actual = await vi.importActual<typeof import("@/api/client")>(
    "@/api/client",
  );
  return { ...actual, putWrapperPermission: vi.fn() };
});

function renderDialog() {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={qc}>
      <EditCellDialog
        storeId="prod"
        clientInstance="ci-1"
        clientInstanceDisplayName="dev-laptop"
        wrapper="gh-authorized"
        resourceId="github/token"
        resourceName="github/token"
        existing={null}
        onClose={() => {}}
      />
    </QueryClientProvider>,
  );
}

describe("EditCellDialog — unsafe-debug-until confirmation flow", () => {
  beforeEach(() => {
    vi.mocked(putWrapperPermission).mockReset();
    vi.mocked(putWrapperPermission).mockResolvedValue(undefined as never);
  });

  it("opens the confirm dialog when picking unsafe-debug-until, and reverts on cancel", async () => {
    renderDialog();
    // Starts Locked: no deadline picker.
    expect(screen.queryByLabelText(/deadline/i)).toBeNull();

    await userEvent.click(screen.getByTestId("mode-radio-unsafe-debug-until"));
    expect(
      await screen.findByTestId("confirm-danger-confirm"),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByTestId("confirm-danger-cancel"));
    await waitFor(() =>
      expect(screen.queryByTestId("confirm-danger-confirm")).toBeNull(),
    );
    // Reverted: still no deadline picker (mode did not switch).
    expect(screen.queryByLabelText(/deadline/i)).toBeNull();
  });

  it("commits the mode on confirm and does not re-prompt when the TTL is edited afterwards", async () => {
    renderDialog();
    await userEvent.click(screen.getByTestId("mode-radio-unsafe-debug-until"));
    await userEvent.click(await screen.findByTestId("confirm-danger-confirm"));

    // Confirmed: deadline picker now visible, confirm dialog gone.
    const deadline = await screen.findByLabelText(/deadline/i);
    expect(screen.queryByTestId("confirm-danger-confirm")).toBeNull();

    // Editing the TTL while already in the mode must not re-open the dialog.
    await userEvent.clear(deadline);
    await userEvent.type(deadline, "2099-01-01T00:00");
    expect(screen.queryByTestId("confirm-danger-confirm")).toBeNull();

    await userEvent.click(screen.getByTestId("permission-save"));
    await waitFor(() =>
      expect(putWrapperPermission).toHaveBeenCalledTimes(1),
    );
    const call = vi.mocked(putWrapperPermission).mock.calls[0];
    // putWrapperPermission(storeId, clientInstance, wrapper, resourceId, body)
    const body = call[4] as { mode: { kind: string } };
    expect(body.mode.kind).toBe("unsafe-debug-until");
  });
});
