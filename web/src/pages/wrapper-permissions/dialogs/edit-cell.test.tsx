import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import type { WrapperPermission } from "@/api/client";
import { deleteWrapperPermission, putWrapperPermission } from "@/api/client";

import { EditCellDialog } from "./edit-cell";

vi.mock("@/api/client", async () => {
  const actual =
    await vi.importActual<typeof import("@/api/client")>("@/api/client");
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
        resourceName="GitHub production token"
        existing={overrides.existing ?? null}
        onClose={overrides.onClose ?? (() => {})}
      />
    </QueryClientProvider>,
  );
}

describe("EditCellDialog — unsafe-debug-until confirmation flow", () => {
  const now = new Date(2035, 3, 5, 6, 7).getTime();
  const pendingLocal = "2035-04-05T06:22";

  beforeEach(() => {
    vi.mocked(putWrapperPermission).mockReset();
    vi.mocked(putWrapperPermission).mockResolvedValue(undefined as never);
    vi.spyOn(Date, "now").mockReturnValue(now);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("pins the complete unsafe-debug security boundary", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByTestId("permission-writable"));
    await user.click(screen.getByTestId("mode-radio-unsafe-debug-until"));

    const warning = await screen.findByTestId("unsafe-debug-security-warning");
    expect(warning).toHaveTextContent(
      "Exact backend authority: select UnsafeDebugUntil for runtime client dev-laptop (id ci-1), store prod, wrapper gh-authorized, and resource GitHub production token (id github/token).",
    );
    expect(warning).toHaveTextContent(
      "Any local user can invoke the system-wide frontend instance entry",
    );
    expect(warning).toHaveTextContent(
      "Backend permission is not Unix-user authorization",
    );
    expect(warning).toHaveTextContent("allowedUsers installs PATH shims only");
    expect(warning).toHaveTextContent("sudo, polkit, or another OS-level gate");
    expect(warning).toHaveTextContent(
      "underwrap-<instance> debug gh-authorized",
    );
    expect(warning).toHaveTextContent(
      "does not know the frontend instance name; verify the matching frontend instance before Save",
    );
    expect(warning).toHaveTextContent(
      "show an environment summary, open an interactive shell, or execute an arbitrary command with the complete configured env, files, and libsecret projection",
    );
    expect(warning).toHaveTextContent(
      "print, copy through accessible paths, or transmit that projection",
    );
    expect(warning).toHaveTextContent("underwrap is not a behavioral sandbox");
    expect(
      within(warning).getByTestId("unsafe-debug-writable-effect"),
    ).toHaveTextContent(
      "files and libsecret changes can be submitted, written back on successful finalization, and persisted for future invocations, affecting their integrity. Env remains backend-owned and immutable.",
    );
    expect(
      within(warning).getByTestId("unsafe-debug-pending-expiry"),
    ).toHaveTextContent(pendingLocal);
    expect(warning).toHaveTextContent("15-minute default");
    expect(warning).toHaveTextContent(
      "Confirming only copies this exact value into the form. Save submits this selection; confirming does not change backend policy",
    );
    expect(warning).toHaveTextContent(
      "Existing permissions and overrides remain in effect until changed or expired",
    );
    expect(warning).toHaveTextContent(
      "backend rechecks expiry on every request",
    );
    expect(putWrapperPermission).not.toHaveBeenCalled();
  });

  it("warns that writable false prevents underwrap persistence, not disclosure", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByTestId("mode-radio-unsafe-debug-until"));

    const effect = await screen.findByTestId("unsafe-debug-readonly-effect");
    expect(effect).toHaveTextContent(
      "this setting does not authorize persistence of staged files or libsecret changes; client overrides and current backend checks still apply",
    );
    expect(effect).toHaveTextContent(
      "Full target access and disclosure are unchanged",
    );
    expect(effect).toHaveTextContent("Env remains backend-owned and immutable");
    expect(putWrapperPermission).not.toHaveBeenCalled();
  });

  it("cancel clears the pending value and preserves the original kind and deadline", async () => {
    const user = userEvent.setup();
    const originalLocal = "2040-02-03T04:05";
    renderDialog({
      existing: {
        client_instance: "ci-1",
        wrapper: "gh-authorized",
        resource_id: "github/token",
        mode: {
          kind: "unlocked-until",
          until_epoch: Math.floor(new Date(originalLocal).getTime() / 1000),
        },
        writable: false,
      },
    });
    expect(screen.getByLabelText(/deadline/i)).toHaveValue(originalLocal);

    await user.click(screen.getByTestId("mode-radio-unsafe-debug-until"));
    expect(
      await screen.findByTestId("unsafe-debug-pending-expiry"),
    ).toHaveTextContent(pendingLocal);
    await user.click(screen.getByTestId("confirm-danger-cancel"));

    await waitFor(() =>
      expect(screen.queryByTestId("unsafe-debug-security-warning")).toBeNull(),
    );
    expect(screen.getByTestId("mode-radio-unlocked-until")).toBeChecked();
    expect(screen.getByLabelText(/deadline/i)).toHaveValue(originalLocal);
    expect(putWrapperPermission).not.toHaveBeenCalled();
  });

  it("confirms the exact displayed deadline locally and saves its epoch and current writable flag", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByTestId("permission-writable"));
    await user.click(screen.getByTestId("mode-radio-unsafe-debug-until"));
    expect(
      await screen.findByTestId("unsafe-debug-pending-expiry"),
    ).toHaveTextContent(pendingLocal);

    await user.click(screen.getByTestId("confirm-danger-confirm"));
    expect(screen.queryByTestId("unsafe-debug-security-warning")).toBeNull();
    expect(screen.getByLabelText(/deadline/i)).toHaveValue(pendingLocal);
    expect(putWrapperPermission).not.toHaveBeenCalled();

    await user.click(screen.getByTestId("permission-save"));
    await waitFor(() => expect(putWrapperPermission).toHaveBeenCalledTimes(1));
    expect(putWrapperPermission).toHaveBeenCalledWith(
      "prod",
      "ci-1",
      "gh-authorized",
      "github/token",
      {
        mode: {
          kind: "unsafe-debug-until",
          until_epoch: Math.floor(new Date(pendingLocal).getTime() / 1000),
        },
        writable: true,
        reason: undefined,
      },
    );
  });

  it("does not re-prompt when editing an existing unsafe-debug deadline", async () => {
    const user = userEvent.setup();
    renderDialog({
      existing: {
        client_instance: "ci-1",
        wrapper: "gh-authorized",
        resource_id: "github/token",
        mode: {
          kind: "unsafe-debug-until",
          until_epoch: Math.floor(
            new Date("2098-01-01T00:00").getTime() / 1000,
          ),
        },
        writable: false,
      },
    });

    const deadline = screen.getByLabelText(/deadline/i);
    await user.clear(deadline);
    await user.type(deadline, "2099-01-01T00:00");
    expect(screen.queryByTestId("unsafe-debug-security-warning")).toBeNull();

    await user.click(screen.getByTestId("permission-save"));
    await waitFor(() => expect(putWrapperPermission).toHaveBeenCalledTimes(1));
    const body = vi.mocked(putWrapperPermission).mock.calls[0][4] as {
      mode: { kind: string; until_epoch: number };
    };
    expect(body.mode).toEqual({
      kind: "unsafe-debug-until",
      until_epoch: Math.floor(new Date("2099-01-01T00:00").getTime() / 1000),
    });
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
    expect(
      await screen.findByText(/pick a valid deadline/i),
    ).toBeInTheDocument();
    expect(putWrapperPermission).not.toHaveBeenCalled();
  });
});
