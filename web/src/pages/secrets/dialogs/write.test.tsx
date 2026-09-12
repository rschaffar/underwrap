import { describe, expect, it, vi, beforeEach } from "vitest";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { fetchSecret, postSecretRename, putSecret } from "@/api/client";

import { WriteSecretDialog } from "./write";
import type { SecretAddress } from "../grouping";

vi.mock("@/api/client", async () => {
  const actual =
    await vi.importActual<typeof import("@/api/client")>("@/api/client");
  return {
    ...actual,
    fetchSecret: vi.fn(),
    putSecret: vi.fn(),
    postSecretRename: vi.fn(),
  };
});

type Mode = "create" | "replace" | "rename";

function renderDialog(
  mode: Mode,
  address: SecretAddress,
  existingKeys: readonly string[] = [],
) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onClose = vi.fn();
  const result = render(
    <QueryClientProvider client={qc}>
      <WriteSecretDialog
        storeId="prod"
        mode={mode}
        address={address}
        resourceName="Deployment credentials"
        existingKeys={existingKeys}
        onClose={onClose}
      />
    </QueryClientProvider>,
  );
  return { ...result, onClose, qc };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function dialogForm() {
  const form = screen.getByRole("dialog").querySelector("form");
  if (!form) throw new Error("Dialog form is missing");
  return form;
}

describe("WriteSecretDialog", () => {
  beforeEach(() => {
    vi.mocked(fetchSecret).mockReset();
    vi.mocked(putSecret).mockReset();
    vi.mocked(putSecret).mockResolvedValue(undefined as never);
    vi.mocked(postSecretRename).mockReset();
    vi.mocked(postSecretRename).mockResolvedValue(undefined as never);
  });

  it("shows the friendly resource name in the heading and its fixed ID and slot secondarily", () => {
    renderDialog("create", { resource_id: "r1", slot: "env", key: "" });

    expect(
      screen.getByRole("heading", {
        name: "Create env entry — Deployment credentials",
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("r1", { selector: "code" })).toBeInTheDocument();
    expect(screen.getByText("env", { selector: "code" })).toBeInTheDocument();
    expect(
      screen.getByText(/Resource and slot stay unchanged/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    expect(fetchSecret).not.toHaveBeenCalled();
  });

  it("creates a new entry with base64-encoded material", async () => {
    const user = userEvent.setup();
    renderDialog("create", { resource_id: "r1", slot: "env", key: "" });
    await user.type(screen.getByLabelText(/^key$/i), "API_TOKEN");
    await user.type(screen.getByTestId("material-input"), "hello");
    await user.click(screen.getByRole("button", { name: /^create$/i }));

    await waitFor(() =>
      expect(putSecret).toHaveBeenCalledWith("prod", "r1", "env", "API_TOKEN", {
        material_b64: btoa("hello"),
      }),
    );
    expect(postSecretRename).not.toHaveBeenCalled();
    expect(fetchSecret).not.toHaveBeenCalled();
  });

  it("creates Unicode material as its exact UTF-8 base64 body", async () => {
    const user = userEvent.setup();
    renderDialog("create", { resource_id: "r1", slot: "files", key: "" });
    await user.type(screen.getByLabelText(/^key$/i), "token.txt");
    await user.type(screen.getByTestId("material-input"), "✓ à la mode");
    await user.click(screen.getByRole("button", { name: /^create$/i }));

    await waitFor(() =>
      expect(putSecret).toHaveBeenCalledWith(
        "prod",
        "r1",
        "files",
        "token.txt",
        { material_b64: "4pyTIMOgIGxhIG1vZGU=" },
      ),
    );
  });

  it("rejects a known duplicate Create key inline and directs to Replace value", async () => {
    const user = userEvent.setup();
    renderDialog("create", { resource_id: "r1", slot: "env", key: "" }, [
      "TAKEN",
    ]);
    await user.type(screen.getByLabelText(/^key$/i), "TAKEN");
    await user.type(screen.getByTestId("material-input"), "new input");
    await user.click(screen.getByRole("button", { name: /^create$/i }));

    expect(
      await screen.findByText(
        "This key already exists. Use Replace value to change its value.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByLabelText(/^key$/i)).toHaveAttribute(
      "aria-invalid",
      "true",
    );
    expect(putSecret).not.toHaveBeenCalled();
    expect(postSecretRename).not.toHaveBeenCalled();

    // A validation rejection must not leave the local submission guard locked.
    await user.clear(screen.getByLabelText(/^key$/i));
    await user.type(screen.getByLabelText(/^key$/i), "AVAILABLE");
    await user.click(screen.getByRole("button", { name: /^create$/i }));
    await waitFor(() => expect(putSecret).toHaveBeenCalledTimes(1));
  });

  it("renames with only to_key, preserving the existing value without revealing it", async () => {
    const user = userEvent.setup();
    renderDialog("rename", { resource_id: "r1", slot: "env", key: "OLD" }, [
      "OLD",
    ]);
    expect(screen.getByText(/Keep existing value/)).toBeInTheDocument();
    expect(screen.queryByTestId("material-input")).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    const key = screen.getByLabelText(/^key$/i);
    await user.clear(key);
    await user.type(key, "NEW");
    await user.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() =>
      expect(postSecretRename).toHaveBeenCalledWith(
        "prod",
        "r1",
        "env",
        "OLD",
        { to_key: "NEW" },
      ),
    );
    expect(postSecretRename).toHaveBeenCalledTimes(1);
    expect(putSecret).not.toHaveBeenCalled();
    expect(fetchSecret).not.toHaveBeenCalled();
  });

  it("rejects a Rename collision without overwriting the destination", async () => {
    const user = userEvent.setup();
    renderDialog(
      "rename",
      { resource_id: "r1", slot: "files", key: "old.txt" },
      ["old.txt", "taken.txt"],
    );
    await user.clear(screen.getByLabelText(/^key$/i));
    await user.type(screen.getByLabelText(/^key$/i), "taken.txt");
    await user.click(screen.getByRole("button", { name: /^save$/i }));

    expect(
      await screen.findByText(
        "This key already exists. Choose a different key.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByLabelText(/^key$/i)).toHaveAttribute(
      "aria-invalid",
      "true",
    );
    expect(postSecretRename).not.toHaveBeenCalled();
    expect(putSecret).not.toHaveBeenCalled();
    expect(fetchSecret).not.toHaveBeenCalled();
  });

  it("disables unchanged Rename Save and rejects a direct unchanged submission", async () => {
    const user = userEvent.setup();
    renderDialog("rename", { resource_id: "r1", slot: "env", key: "OLD" }, [
      "OLD",
    ]);
    const save = screen.getByRole("button", { name: /^save$/i });
    expect(save).toBeDisabled();
    await user.type(screen.getByLabelText(/^key$/i), "_NEW");
    expect(save).toBeEnabled();
    await user.clear(screen.getByLabelText(/^key$/i));
    await user.type(screen.getByLabelText(/^key$/i), "OLD");
    expect(save).toBeDisabled();

    fireEvent.submit(dialogForm());
    expect(
      await screen.findByText("Choose a different key to rename this entry."),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/This key already exists/),
    ).not.toBeInTheDocument();
    expect(postSecretRename).not.toHaveBeenCalled();
    expect(putSecret).not.toHaveBeenCalled();
  });

  it("replaces Unicode material at the pinned key without fetching the previous value", async () => {
    const user = userEvent.setup();
    renderDialog(
      "replace",
      { resource_id: "r1", slot: "env", key: "EXISTING" },
      ["EXISTING"],
    );
    const key = screen.getByLabelText(/^key$/i);
    expect(key).toHaveAttribute("readonly");
    expect(screen.getByTestId("material-input")).toHaveValue("");
    await user.type(key, "_CHANGED");
    expect(key).toHaveValue("EXISTING");
    await user.type(screen.getByTestId("material-input"), "päss🔐");
    await user.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() =>
      expect(putSecret).toHaveBeenCalledWith("prod", "r1", "env", "EXISTING", {
        material_b64: "cMOkc3Pwn5SQ",
      }),
    );
    expect(postSecretRename).not.toHaveBeenCalled();
    expect(fetchSecret).not.toHaveBeenCalled();
  });

  it.each(["create", "replace"] as const)(
    "stores explicitly empty %s material as an empty base64 string",
    async (mode) => {
      const user = userEvent.setup();
      renderDialog(mode, {
        resource_id: "r1",
        slot: "files",
        key: mode === "create" ? "" : "entry.txt",
      });
      if (mode === "create")
        await user.type(screen.getByLabelText(/^key$/i), "entry.txt");
      const material = screen.getByTestId("material-input");
      await user.type(material, "discard this newly supplied input");
      await user.click(
        screen.getByRole("checkbox", { name: "Store an empty value" }),
      );
      expect(material).toBeDisabled();
      expect(material).toHaveValue("");
      await user.click(
        screen.getByRole("button", {
          name: mode === "create" ? "Create" : "Save",
        }),
      );

      await waitFor(() =>
        expect(putSecret).toHaveBeenCalledWith(
          "prod",
          "r1",
          "files",
          "entry.txt",
          { material_b64: "" },
        ),
      );
      expect(postSecretRename).not.toHaveBeenCalled();
      expect(fetchSecret).not.toHaveBeenCalled();
    },
  );

  it.each(["create", "replace"] as const)(
    "rejects untouched blank %s material",
    async (mode) => {
      const user = userEvent.setup();
      renderDialog(mode, {
        resource_id: "r1",
        slot: "env",
        key: mode === "create" ? "" : "EXISTING",
      });
      if (mode === "create")
        await user.type(screen.getByLabelText(/^key$/i), "NEW");
      expect(
        screen.getByRole("checkbox", { name: "Store an empty value" }),
      ).not.toBeChecked();
      await user.click(
        screen.getByRole("button", {
          name: mode === "create" ? "Create" : "Save",
        }),
      );

      expect(
        await screen.findByText(
          "Supply a new value or select Store an empty value.",
        ),
      ).toBeInTheDocument();
      expect(putSecret).not.toHaveBeenCalled();
      expect(postSecretRename).not.toHaveBeenCalled();
    },
  );

  it("requires new input again after explicit-empty consent is removed", async () => {
    const user = userEvent.setup();
    renderDialog("replace", {
      resource_id: "r1",
      slot: "env",
      key: "EXISTING",
    });
    const checkbox = screen.getByRole("checkbox", {
      name: "Store an empty value",
    });
    await user.click(checkbox);
    await user.click(checkbox);
    expect(screen.getByTestId("material-input")).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(
      await screen.findByText(
        "Supply a new value or select Store an empty value.",
      ),
    ).toBeInTheDocument();
    expect(putSecret).not.toHaveBeenCalled();
  });

  it.each(["create", "replace", "rename"] as const)(
    "blocks dismissal, inputs, and duplicate %s submissions while pending",
    async (mode) => {
      const user = userEvent.setup();
      const request = deferred<Awaited<ReturnType<typeof putSecret>>>();
      vi.mocked(putSecret).mockReturnValueOnce(request.promise);
      vi.mocked(postSecretRename).mockReturnValueOnce(request.promise);
      const { onClose } = renderDialog(mode, {
        resource_id: "r1",
        slot: "env",
        key: mode === "create" ? "" : "OLD",
      });
      if (mode !== "replace") {
        await user.clear(screen.getByLabelText(/^key$/i));
        await user.type(screen.getByLabelText(/^key$/i), "NEW");
      }
      if (mode !== "rename")
        await user.type(screen.getByTestId("material-input"), "new input");

      // Two immediate events also exercise the guard before React renders pending.
      fireEvent.submit(dialogForm());
      fireEvent.submit(dialogForm());
      const mutation = mode === "rename" ? postSecretRename : putSecret;
      await waitFor(() => expect(mutation).toHaveBeenCalledTimes(1));
      expect(screen.getByLabelText(/^key$/i)).toBeDisabled();
      expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
      expect(screen.getByRole("button", { name: "Saving…" })).toBeDisabled();
      if (mode !== "rename") {
        expect(screen.getByTestId("material-input")).toBeDisabled();
        expect(
          screen.getByRole("checkbox", { name: "Store an empty value" }),
        ).toBeDisabled();
      }
      await user.keyboard("{Escape}");
      await user.click(screen.getByRole("button", { name: "Close" }));
      await user.click(screen.getByRole("button", { name: "Cancel" }));
      fireEvent.pointerDown(document.body, { pointerType: "mouse", button: 0 });
      fireEvent.focusIn(document.body);
      fireEvent.submit(dialogForm());
      expect(onClose).not.toHaveBeenCalled();
      expect(screen.getByRole("dialog")).toBeInTheDocument();
      expect(mutation).toHaveBeenCalledTimes(1);
      expect(fetchSecret).not.toHaveBeenCalled();

      await act(async () => request.resolve(undefined as never));
      await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    },
  );

  it("retains the pending guard through query invalidation", async () => {
    const user = userEvent.setup();
    const { onClose, qc } = renderDialog("replace", {
      resource_id: "r1",
      slot: "env",
      key: "OLD",
    });
    const invalidation = deferred<void>();
    const invalidate = vi
      .spyOn(qc, "invalidateQueries")
      .mockReturnValueOnce(invalidation.promise);
    await user.type(screen.getByTestId("material-input"), "new input");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(invalidate).toHaveBeenCalled());
    expect(screen.getByRole("button", { name: "Saving…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    await user.keyboard("{Escape}");
    fireEvent.submit(dialogForm());
    expect(putSecret).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();

    await act(async () => invalidation.resolve());
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("unlocks controls after a request failure and permits an explicit retry", async () => {
    const user = userEvent.setup();
    const request = deferred<Awaited<ReturnType<typeof putSecret>>>();
    vi.mocked(putSecret).mockReturnValueOnce(request.promise);
    const { onClose } = renderDialog("replace", {
      resource_id: "r1",
      slot: "env",
      key: "OLD",
    });
    await user.type(screen.getByTestId("material-input"), "new input");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await act(async () =>
      request.reject(new Error("Synthetic request failure")),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Save" })).toBeEnabled(),
    );
    expect(screen.getByRole("button", { name: "Cancel" })).toBeEnabled();
    expect(screen.getByTestId("material-input")).toBeEnabled();
    expect(onClose).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(putSecret).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });
});
