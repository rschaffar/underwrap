import type { ComponentProps, MouseEvent } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  QueryClient,
  QueryClientProvider,
  QueryObserver,
} from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { deleteWrapperPermission, putWrapperPermission } from "@/api/client";
import { Button } from "@/components/ui/button";
import { EditCellDialog } from "./edit-cell";

vi.mock("@/api/client", async () => ({
  ...(await vi.importActual<typeof import("@/api/client")>("@/api/client")),
  putWrapperPermission: vi.fn(),
  deleteWrapperPermission: vi.fn(),
}));
const callbacks = vi.hoisted(() => ({
  save: undefined as undefined | (() => unknown),
  remove: undefined as undefined | (() => unknown),
}));
vi.mock("@/components/ui/button", async () => {
  const actual = await vi.importActual<typeof import("@/components/ui/button")>(
    "@/components/ui/button",
  );
  return {
    ...actual,
    Button: (
      props: ComponentProps<typeof Button> & { "data-testid"?: string },
    ) => {
      const call = () => props.onClick?.({} as MouseEvent<HTMLButtonElement>);
      if (props["data-testid"] === "permission-save") callbacks.save = call;
      if (props["data-testid"] === "permission-remove") callbacks.remove = call;
      return <actual.Button {...props} />;
    },
  };
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
function renderDialog(
  overrides: Partial<ComponentProps<typeof EditCellDialog>> = {},
) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const onClose = vi.fn();
  const view = render(
    <QueryClientProvider client={qc}>
      <EditCellDialog
        storeId="store_test"
        clientInstance="client_test"
        clientInstanceDisplayName="Test client"
        wrapper="test-wrapper"
        resourceId="res_test"
        resourceName="Test resource"
        existing={{
          client_instance: "client_test",
          wrapper: "test-wrapper",
          resource_id: "res_test",
          mode: { kind: "locked" },
          writable: false,
        }}
        accessOverride={{ kind: "individual" }}
        onClose={onClose}
        {...overrides}
      />
    </QueryClientProvider>,
  );
  return { qc, onClose, unmount: view.unmount };
}
beforeEach(() => {
  vi.mocked(putWrapperPermission)
    .mockReset()
    .mockResolvedValue(undefined as never);
  vi.mocked(deleteWrapperPermission)
    .mockReset()
    .mockResolvedValue(undefined as never);
  callbacks.save = undefined;
  callbacks.remove = undefined;
});

describe("permission mutation exclusion", () => {
  it.each(["save", "remove"] as const)(
    "preserves actual refresh rejection after %s without a false request error",
    async (operation) => {
      const view = renderDialog();
      const failure = new Error("permission view refresh failed");
      const queryKey = ["admin", "wrapper-permissions-view", "store_test"];
      view.qc.setQueryData(queryKey, {
        permissions: [],
        pairs: [],
        resources: [],
      });
      const queryFn = vi.fn().mockRejectedValue(failure);
      const observer = new QueryObserver(view.qc, {
        queryKey,
        queryFn,
        staleTime: Infinity,
      });
      const unsubscribe = observer.subscribe(() => {});
      try {
        expect(queryFn).not.toHaveBeenCalled();
        await act(async () => {
          await expect(Promise.resolve(callbacks[operation]!())).rejects.toBe(
            failure,
          );
        });
        expect(queryFn).toHaveBeenCalledOnce();
        expect(
          operation === "save" ? putWrapperPermission : deleteWrapperPermission,
        ).toHaveBeenCalledOnce();
        expect(view.onClose).not.toHaveBeenCalled();
        expect(screen.queryByRole("alert")).not.toBeInTheDocument();
        expect(screen.getByTestId("permission-save")).toBeEnabled();
        expect(view.qc.getQueryState(queryKey)?.status).toBe("error");
      } finally {
        unsubscribe();
        view.unmount();
        view.qc.clear();
      }
    },
  );
  it.each(["save", "remove"] as const)(
    "freezes both actions and fields while %s is pending",
    async (operation) => {
      const request = deferred<never>();
      const api =
        operation === "save" ? putWrapperPermission : deleteWrapperPermission;
      vi.mocked(api).mockReturnValue(request.promise);
      const { onClose } = renderDialog();
      let pending: unknown;
      act(() => {
        pending = callbacks[operation]!();
        void callbacks.save!();
        void callbacks.remove!();
      });
      await waitFor(() => expect(api).toHaveBeenCalledTimes(1));
      expect(
        operation === "save" ? deleteWrapperPermission : putWrapperPermission,
      ).not.toHaveBeenCalled();
      for (const id of [
        "permission-save",
        "permission-remove",
        "permission-writable",
        "mode-radio-unlocked",
      ])
        expect(screen.getByTestId(id)).toBeDisabled();
      expect(screen.getByLabelText("Reason (optional)")).toBeDisabled();
      expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
      expect(screen.getByRole("button", { name: "Close" })).toBeDisabled();
      await userEvent.setup().keyboard("{Escape}");
      expect(onClose).not.toHaveBeenCalled();
      await act(async () => {
        request.resolve(undefined as never);
        await pending;
      });
      expect(onClose).toHaveBeenCalledTimes(1);
    },
  );

  it("releases repeated request failures for deliberate retry", async () => {
    vi.mocked(putWrapperPermission).mockRejectedValue(
      new Error("synthetic request failure"),
    );
    const { onClose } = renderDialog();
    for (let attempt = 1; attempt <= 2; attempt++) {
      await act(async () => {
        await callbacks.save!();
      });
      expect(putWrapperPermission).toHaveBeenCalledTimes(attempt);
      expect(screen.getByRole("alert")).toHaveTextContent(/network error/i);
      expect(screen.getByTestId("permission-save")).toBeEnabled();
      expect(screen.getByTestId("permission-remove")).toBeEnabled();
    }
    expect(onClose).not.toHaveBeenCalled();
  });

  it.each(["invalidation", "callback"])(
    "does not mislabel %s failure as a request failure",
    async (phase) => {
      const failure = new Error(`synthetic ${phase} failure`);
      const view = renderDialog(
        phase === "callback"
          ? {
              onClose: () => {
                throw failure;
              },
            }
          : {},
      );
      if (phase === "invalidation")
        vi.spyOn(view.qc, "invalidateQueries").mockRejectedValue(failure);
      await act(async () => {
        await expect(Promise.resolve(callbacks.save!())).rejects.toBe(failure);
      });
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(screen.getByTestId("permission-save")).toBeEnabled();
      expect(putWrapperPermission).toHaveBeenCalledTimes(1);
    },
  );

  it.each([false, true])(
    "warns about override writable=%s without rewriting the saved flag",
    async (writable) => {
      renderDialog({
        accessOverride: { kind: "unlocked", writable },
        existing: {
          client_instance: "client_test",
          wrapper: "test-wrapper",
          resource_id: "res_test",
          mode: { kind: "locked" },
          writable: !writable,
        },
      });
      fireEvent.click(screen.getByTestId("mode-radio-unsafe-debug-until"));
      expect(
        await screen.findByTestId(
          writable
            ? "unsafe-debug-writable-effect"
            : "unsafe-debug-readonly-effect",
        ),
      ).toBeInTheDocument();
      expect(
        screen.getByTestId("unsafe-debug-security-warning"),
      ).toHaveTextContent(
        /Unlock all overrides ordinary access and the saved writable flag/i,
      );
      fireEvent.click(screen.getByTestId("confirm-danger-confirm"));
      await act(async () => {
        await callbacks.save!();
      });
      expect(vi.mocked(putWrapperPermission).mock.calls[0][4].writable).toBe(
        !writable,
      );
    },
  );
});
