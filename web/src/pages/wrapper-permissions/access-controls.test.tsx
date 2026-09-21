import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  type AdminClientInstanceListItem,
  type AdminClientInstancesListResponse,
  deleteWrapperPermission,
  fetchClientInstances,
  putClientInstanceUpdate,
  putWrapperPermission,
} from "@/api/client";
import { ConfirmDialog } from "@/components/confirm-dialog";

import { ClientAccessControls } from "./access-controls";

vi.mock("@/api/client", async () => {
  const actual =
    await vi.importActual<typeof import("@/api/client")>("@/api/client");
  return {
    ...actual,
    fetchClientInstances: vi.fn(),
    putClientInstanceUpdate: vi.fn(),
    putWrapperPermission: vi.fn(),
    deleteWrapperPermission: vi.fn(),
  };
});

// Render the actual dialog; expose its async callback only for direct rejection
// and same-turn duplicate assertions, without global rejection suppression.
vi.mock("@/components/confirm-dialog", async () => {
  const actual = await vi.importActual<
    typeof import("@/components/confirm-dialog")
  >("@/components/confirm-dialog");
  return { ...actual, ConfirmDialog: vi.fn(actual.ConfirmDialog) };
});

type AccessOverride = AdminClientInstanceListItem["access_override"];
const clientsKey = ["admin", "client-instances"] as const;
const permissionsKey = ["admin", "wrapper-permissions-view"] as const;

function item(
  access_override: AccessOverride = { kind: "individual" },
): AdminClientInstanceListItem {
  return {
    id: "client_controls",
    display_name: "Studio workstation",
    current_store_id: "store_studio",
    current_store_name: "Studio",
    status: "active",
    current_credential_fingerprint: "synthetic-metadata-only",
    auto_create_wrapper_assets: false,
    access_override,
    actions: { can_disable: true, can_enable: false },
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function renderControls(
  instance = item(),
  onUncertaintyChange?: (clientId: string, unknown: boolean) => void,
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  queryClient.setQueryData(clientsKey, { items: [instance] });
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  const view = render(
    <QueryClientProvider client={queryClient}>
      <ClientAccessControls
        instance={instance}
        onUncertaintyChange={onUncertaintyChange}
      />
    </QueryClientProvider>,
  );
  return { ...view, queryClient, invalidate };
}

// The underlying control remains in the DOM while Radix correctly hides it
// from the accessibility tree behind the confirmation dialog.
const selector = () => screen.getByLabelText("Client access");
const writeback = () =>
  screen.getByRole("checkbox", { name: "Allow files/libsecret writeback" });

async function openUnlock(user: ReturnType<typeof userEvent.setup>) {
  await user.selectOptions(selector(), "unlocked");
  return screen.getByRole("dialog", { name: "Unlock all: Studio workstation" });
}

function confirmationCallback() {
  const calls = vi.mocked(ConfirmDialog).mock.calls;
  return calls[calls.length - 1][0].onConfirm;
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(fetchClientInstances).mockReset();
  vi.mocked(fetchClientInstances).mockResolvedValue({ items: [item()] });
  vi.mocked(putClientInstanceUpdate).mockReset();
  vi.mocked(putClientInstanceUpdate).mockImplementation(async (_, body) =>
    item(body.access_override ?? { kind: "individual" }),
  );
});

afterEach(() => {
  expect(putWrapperPermission).not.toHaveBeenCalled();
  expect(deleteWrapperPermission).not.toHaveBeenCalled();
  vi.restoreAllMocks();
});

describe("ClientAccessControls", () => {
  it.each([
    [{ kind: "individual" }, "locked"],
    [{ kind: "locked" }, "individual"],
    [{ kind: "unlocked", writable: true }, "individual"],
    [{ kind: "unlocked", writable: true }, "locked"],
  ] satisfies Array<[AccessOverride, "individual" | "locked"]>)(
    "applies %j → %s immediately, without changing the confirmed selection early",
    async (initial, next) => {
      const request = deferred<AdminClientInstanceListItem>();
      vi.mocked(putClientInstanceUpdate).mockReturnValue(request.promise);
      const { queryClient, invalidate } = renderControls(item(initial));
      const user = userEvent.setup();

      await user.selectOptions(selector(), next);

      expect(putClientInstanceUpdate).toHaveBeenCalledExactlyOnceWith(
        "client_controls",
        {
          access_override: { kind: next },
        },
      );
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      expect(selector()).toHaveValue(initial.kind);
      expect(selector()).toBeDisabled();
      if (initial.kind === "unlocked") {
        expect(writeback()).toBeChecked();
        expect(writeback()).toBeDisabled();
      }
      expect(queryClient.getQueryData(clientsKey)).toEqual({
        items: [item(initial)],
      });

      await act(async () => request.resolve(item({ kind: next })));

      await waitFor(() => expect(selector()).toBeEnabled());
      expect(selector()).toHaveValue(next);
      expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
      expect(queryClient.getQueryData(clientsKey)).toEqual({
        items: [item({ kind: next })],
      });
      expect(invalidate).toHaveBeenCalledWith(
        { queryKey: clientsKey },
        { throwOnError: true },
      );
      expect(invalidate).toHaveBeenCalledWith(
        { queryKey: permissionsKey },
        { throwOnError: true },
      );
    },
  );

  it.each([
    { kind: "individual" },
    { kind: "locked" },
    { kind: "unlocked", writable: true },
  ] satisfies AccessOverride[])(
    "does nothing when %j is already active",
    async (access) => {
      renderControls(item(access));
      await userEvent.setup().selectOptions(selector(), access.kind);
      expect(putClientInstanceUpdate).not.toHaveBeenCalled();
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    },
  );

  it("requires confirmation only for entering Unlock, and Cancel sends nothing", async () => {
    renderControls(item({ kind: "locked" }));
    const user = userEvent.setup();
    const dialog = await openUnlock(user);

    expect(selector()).toHaveValue("locked");
    expect(putClientInstanceUpdate).not.toHaveBeenCalled();
    expect(dialog).toHaveTextContent("Studio workstation in store Studio");
    expect(dialog).toHaveTextContent(/all registered wrapper\/resource pairs/i);
    expect(dialog).toHaveTextContent(
      /missing, locked, or expired ordinary permission rows/i,
    );
    expect(dialog).toHaveTextContent(/saved individual rows stay unchanged/i);
    expect(dialog).toHaveTextContent(
      /unsafe debug still needs active timed per-pair approval/i,
    );
    expect(dialog).toHaveTextContent(/initial override is read-only/i);
    expect(dialog).toHaveTextContent(/env cannot be mutated/i);
    expect(dialog).toHaveTextContent(
      /disabled identities, locked stores, and unregistered pairs are not bypassed/i,
    );

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));

    expect(putClientInstanceUpdate).not.toHaveBeenCalled();
    expect(selector()).toHaveValue("locked");
    expect(selector()).toBeEnabled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(
      screen.getByText(/changes apply on subsequent opens and finalizes/i),
    ).toHaveTextContent(/writable staging is not persistence permission/i);
  });

  it("initially unlocks read-only and freezes controls until the server response", async () => {
    const request = deferred<AdminClientInstanceListItem>();
    vi.mocked(putClientInstanceUpdate).mockReturnValue(request.promise);
    renderControls();
    const user = userEvent.setup();
    const dialog = await openUnlock(user);
    await user.click(
      within(dialog).getByRole("button", { name: "Unlock all read-only" }),
    );

    expect(putClientInstanceUpdate).toHaveBeenCalledExactlyOnceWith(
      "client_controls",
      {
        access_override: { kind: "unlocked", writable: false },
      },
    );
    expect(selector()).toHaveValue("individual");
    expect(selector()).toBeDisabled();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(
      within(dialog).getByRole("button", { name: "Unlocking…" }),
    ).toBeDisabled();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(dialog).toBeInTheDocument();

    await act(async () =>
      request.resolve(item({ kind: "unlocked", writable: false })),
    );

    await waitFor(() => expect(selector()).toBeEnabled());
    expect(selector()).toHaveValue("unlocked");
    expect(writeback()).not.toBeChecked();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it.each([false, true])(
    "toggles writable=%s immediately without another confirmation",
    async (writable) => {
      const request = deferred<AdminClientInstanceListItem>();
      vi.mocked(putClientInstanceUpdate).mockReturnValue(request.promise);
      renderControls(item({ kind: "unlocked", writable }));
      const user = userEvent.setup();
      await user.click(writeback());

      expect(putClientInstanceUpdate).toHaveBeenCalledExactlyOnceWith(
        "client_controls",
        {
          access_override: { kind: "unlocked", writable: !writable },
        },
      );
      expect(writeback()).toHaveProperty("checked", writable);
      expect(writeback()).toBeDisabled();
      expect(selector()).toBeDisabled();
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

      await act(async () =>
        request.resolve(item({ kind: "unlocked", writable: !writable })),
      );

      await waitFor(() => expect(writeback()).toBeEnabled());
      expect(writeback()).toHaveProperty("checked", !writable);
    },
  );

  it("guards same-turn confirmation callbacks synchronously", async () => {
    const request = deferred<AdminClientInstanceListItem>();
    vi.mocked(putClientInstanceUpdate).mockReturnValue(request.promise);
    renderControls();
    await openUnlock(userEvent.setup());
    const confirm = confirmationCallback();
    let first: unknown;
    let second: unknown;
    act(() => {
      first = confirm();
      second = confirm();
    });
    await act(async () => {
      await second;
    });
    expect(putClientInstanceUpdate).toHaveBeenCalledTimes(1);
    await act(async () => {
      request.resolve(item({ kind: "unlocked", writable: false }));
      await first;
    });
    expect(selector()).toHaveValue("unlocked");
  });

  it("blocks simultaneous checkbox and selector changes while one request is in flight", async () => {
    const request = deferred<AdminClientInstanceListItem>();
    vi.mocked(putClientInstanceUpdate).mockReturnValue(request.promise);
    renderControls(item({ kind: "unlocked", writable: false }));
    act(() => {
      fireEvent.click(writeback());
      fireEvent.change(selector(), { target: { value: "locked" } });
      fireEvent.click(writeback());
    });
    await waitFor(() =>
      expect(putClientInstanceUpdate).toHaveBeenCalledTimes(1),
    );
    expect(putClientInstanceUpdate).toHaveBeenCalledWith("client_controls", {
      access_override: { kind: "unlocked", writable: true },
    });
    await act(async () =>
      request.resolve(item({ kind: "unlocked", writable: true })),
    );
    expect(selector()).toHaveValue("unlocked");
  });

  it("uses the returned server state instead of assuming the requested state", async () => {
    vi.mocked(putClientInstanceUpdate).mockResolvedValue(
      item({ kind: "individual" }),
    );
    renderControls();
    await userEvent.setup().selectOptions(selector(), "locked");
    await waitFor(() => expect(selector()).toBeEnabled());
    expect(selector()).toHaveValue("individual");
  });

  it("reconciles an ambiguous failure with a fresh list before showing current access", async () => {
    const request = deferred<AdminClientInstanceListItem>();
    const refresh = deferred<AdminClientInstancesListResponse>();
    vi.mocked(putClientInstanceUpdate).mockReturnValue(request.promise);
    vi.mocked(fetchClientInstances).mockReturnValue(refresh.promise);
    const { queryClient } = renderControls();
    const user = userEvent.setup();
    const dialog = await openUnlock(user);
    await user.click(
      within(dialog).getByRole("button", { name: "Unlock all read-only" }),
    );
    await act(async () => request.reject(new Error("synthetic lost response")));

    await waitFor(() => expect(fetchClientInstances).toHaveBeenCalledTimes(1));
    expect(selector()).toHaveValue("unknown");
    expect(selector()).toBeDisabled();
    expect(
      screen.getByText(/client access state is unknown/i),
    ).toBeInTheDocument();
    expect(screen.getByText(/network error/i)).toBeInTheDocument();

    const refreshed = item({ kind: "unlocked", writable: false });
    await act(async () => refresh.resolve({ items: [refreshed] }));

    await waitFor(() => expect(selector()).toBeEnabled());
    expect(selector()).toHaveValue("unlocked");
    expect(writeback()).not.toBeChecked();
    expect(
      screen.queryByText(/client access state is unknown/i),
    ).not.toBeInTheDocument();
    expect(queryClient.getQueryData(clientsKey)).toEqual({
      items: [refreshed],
    });
    expect(putClientInstanceUpdate).toHaveBeenCalledTimes(1);
  });

  it("handles a synchronous request throw and reconciles without retrying the mutation", async () => {
    vi.mocked(putClientInstanceUpdate).mockImplementation(() => {
      throw new Error("synthetic request throw");
    });
    renderControls();
    await openUnlock(userEvent.setup());
    await expect(
      act(async () => {
        await confirmationCallback()();
      }),
    ).resolves.toBeUndefined();
    expect(fetchClientInstances).toHaveBeenCalledTimes(1);
    expect(putClientInstanceUpdate).toHaveBeenCalledTimes(1);
    expect(selector()).toHaveValue("individual");
    expect(selector()).toBeEnabled();
    expect(screen.getByRole("alert")).toHaveTextContent(/network error/i);
  });

  it.each(["unavailable", "missing"])(
    "keeps %s reconciliation unknown until an explicit non-overlapping retry refresh",
    async (failure) => {
      vi.mocked(putClientInstanceUpdate).mockRejectedValue(
        new Error("synthetic request failure"),
      );
      if (failure === "unavailable") {
        vi.mocked(fetchClientInstances).mockRejectedValueOnce(
          new Error("synthetic refresh failure"),
        );
      } else {
        vi.mocked(fetchClientInstances).mockResolvedValueOnce({ items: [] });
      }
      renderControls(item({ kind: "unlocked", writable: true }));
      const user = userEvent.setup();
      await user.selectOptions(selector(), "locked");
      const retry = await screen.findByRole("button", {
        name: "Retry refresh",
      });
      await waitFor(() => expect(retry).toBeEnabled());
      expect(selector()).toHaveValue("unknown");
      expect(selector()).toBeDisabled();
      expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
      expect(fetchClientInstances).toHaveBeenCalledTimes(1);

      const refresh = deferred<AdminClientInstancesListResponse>();
      vi.mocked(fetchClientInstances).mockReturnValueOnce(refresh.promise);
      act(() => {
        fireEvent.click(retry);
        fireEvent.click(retry);
        fireEvent.change(selector(), { target: { value: "individual" } });
      });
      await waitFor(() =>
        expect(fetchClientInstances).toHaveBeenCalledTimes(2),
      );
      expect(retry).toBeDisabled();
      expect(selector()).toHaveValue("unknown");
      expect(putClientInstanceUpdate).toHaveBeenCalledTimes(1);

      await act(async () =>
        refresh.resolve({ items: [item({ kind: "locked" })] }),
      );
      await waitFor(() => expect(selector()).toBeEnabled());
      expect(selector()).toHaveValue("locked");
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(putClientInstanceUpdate).toHaveBeenCalledTimes(1);
    },
  );

  it("reports uncertainty to its column until authoritative reconciliation completes", async () => {
    const notify = vi.fn();
    const refresh = deferred<AdminClientInstancesListResponse>();
    vi.mocked(putClientInstanceUpdate).mockRejectedValue(
      new Error("response lost"),
    );
    vi.mocked(fetchClientInstances).mockReturnValue(refresh.promise);
    const { unmount } = renderControls(item(), notify);
    expect(notify).toHaveBeenLastCalledWith("client_controls", false);
    await userEvent.setup().selectOptions(selector(), "locked");
    await waitFor(() =>
      expect(notify).toHaveBeenLastCalledWith("client_controls", true),
    );
    await act(async () =>
      refresh.resolve({ items: [item({ kind: "locked" })] }),
    );
    await waitFor(() => expect(selector()).toHaveValue("locked"));
    expect(notify).toHaveBeenLastCalledWith("client_controls", false);
    unmount();
    expect(notify).toHaveBeenLastCalledWith("client_controls", false);
  });

  it("clears its column marker on unmount and ignores late parent notifications", async () => {
    const notify = vi.fn();
    const request = deferred<AdminClientInstanceListItem>();
    vi.mocked(putClientInstanceUpdate).mockReturnValue(request.promise);
    const { unmount } = renderControls(item(), notify);
    await userEvent.setup().selectOptions(selector(), "locked");
    unmount();
    expect(notify).toHaveBeenLastCalledWith("client_controls", false);
    const calls = notify.mock.calls.length;
    await act(async () => request.reject(new Error("late response loss")));
    expect(fetchClientInstances).toHaveBeenCalledOnce();
    expect(notify).toHaveBeenCalledTimes(calls);
  });

  it("keeps invalidation failure observable, separate from a successful update", async () => {
    const failure = new Error("synthetic invalidation failure");
    const { invalidate } = renderControls();
    invalidate.mockRejectedValue(failure);
    await openUnlock(userEvent.setup());
    const confirm = confirmationCallback();

    await act(async () => {
      await expect(Promise.resolve(confirm())).rejects.toBe(failure);
    });

    expect(fetchClientInstances).not.toHaveBeenCalled();
    expect(putClientInstanceUpdate).toHaveBeenCalledTimes(1);
    expect(selector()).toHaveValue("unlocked");
    expect(selector()).toBeEnabled();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(invalidate).toHaveBeenCalledTimes(2);
  });
});
