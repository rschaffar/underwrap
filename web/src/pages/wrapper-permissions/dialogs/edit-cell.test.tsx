import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import type { WrapperPermission } from "@/api/client";
import { deleteWrapperPermission, putWrapperPermission } from "@/api/client";

import { EditCellDialog } from "./edit-cell";

vi.mock("@/api/client", async () => {
  const actual = await vi.importActual<typeof import("@/api/client")>(
    "@/api/client",
  );
  return {
    ...actual,
    putWrapperPermission: vi.fn(),
    deleteWrapperPermission: vi.fn(),
  };
});

function renderDialog(
  overrides: { existing?: WrapperPermission | null; onClose?: () => void } = {},
) {
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
        existing={overrides.existing ?? null}
        onClose={overrides.onClose ?? (() => {})}
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

describe("EditCellDialog — existing permission, writable, reason, remove", () => {
  beforeEach(() => {
    vi.mocked(putWrapperPermission).mockReset();
    vi.mocked(putWrapperPermission).mockResolvedValue(undefined as never);
    vi.mocked(deleteWrapperPermission).mockReset();
    vi.mocked(deleteWrapperPermission).mockResolvedValue(undefined as never);
  });

  const existing: WrapperPermission = {
    client_instance: "ci-1",
    wrapper: "gh-authorized",
    resource_id: "github/token",
    mode: { kind: "unlocked" },
    writable: true,
    reason: "ops handoff",
  };

  it("pre-fills mode, writable, and reason from an existing permission", async () => {
    renderDialog({ existing });
    expect(await screen.findByTestId("permission-writable")).toBeChecked();
    expect(screen.getByLabelText(/reason/i)).toHaveValue("ops handoff");
    // Unlocked mode has no deadline picker.
    expect(screen.queryByLabelText(/deadline/i)).toBeNull();
  });

  it("saves a locked mode with writable and reason in the body", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByTestId("permission-writable"));
    await user.type(screen.getByLabelText(/reason/i), "grant write");
    await user.click(screen.getByTestId("permission-save"));
    await waitFor(() => expect(putWrapperPermission).toHaveBeenCalledTimes(1));
    const body = vi.mocked(putWrapperPermission).mock.calls[0][4] as {
      mode: { kind: string };
      writable: boolean;
      reason?: string;
    };
    expect(body.mode.kind).toBe("locked");
    expect(body.writable).toBe(true);
    expect(body.reason).toBe("grant write");
  });

  it("saves an unlocked mode without re-prompting", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByTestId("mode-radio-unlocked"));
    await user.click(screen.getByTestId("permission-save"));
    await waitFor(() => expect(putWrapperPermission).toHaveBeenCalledTimes(1));
    const body = vi.mocked(putWrapperPermission).mock.calls[0][4] as {
      mode: { kind: string };
    };
    expect(body.mode.kind).toBe("unlocked");
  });

  it("removes the permission via deleteWrapperPermission", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    renderDialog({ existing, onClose });
    await user.click(await screen.findByTestId("permission-remove"));
    await waitFor(() =>
      expect(deleteWrapperPermission).toHaveBeenCalledWith(
        "prod",
        "ci-1",
        "gh-authorized",
        "github/token",
      ),
    );
  });

  it("rejects an empty deadline on unlocked-until", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByTestId("mode-radio-unlocked-until"));
    const deadline = await screen.findByLabelText(/deadline/i);
    await user.clear(deadline);
    await user.click(screen.getByTestId("permission-save"));
    expect(await screen.findByText(/pick a valid deadline/i)).toBeInTheDocument();
    expect(putWrapperPermission).not.toHaveBeenCalled();
  });
});
