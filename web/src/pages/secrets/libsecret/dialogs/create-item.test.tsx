import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { postLibsecretItem } from "@/api/client";

import { CreateItemDialog } from "./create-item";

vi.mock("@/api/client", async () => {
  const actual = await vi.importActual<typeof import("@/api/client")>(
    "@/api/client",
  );
  return { ...actual, postLibsecretItem: vi.fn() };
});

function renderDialog(existingIds: string[] = []) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <CreateItemDialog
        storeId="prod"
        resourceId="r1"
        collectionId="default"
        existingIds={existingIds}
        onClose={() => {}}
      />
    </QueryClientProvider>,
  );
}

describe("CreateItemDialog", () => {
  beforeEach(() => {
    vi.mocked(postLibsecretItem).mockReset();
    vi.mocked(postLibsecretItem).mockResolvedValue(undefined as never);
  });

  it.each([
    ["malformed", "service=github\nmissing", "Line 2 must use key=value."],
    [
      "empty key",
      "service=github\n = value",
      "Line 2 has an empty attribute key.",
    ],
    [
      "duplicate key",
      "service=github\n\n service = gitlab",
      'Line 3 repeats attribute key "service" from line 1.',
    ],
  ])(
    "rejects %s attributes inline without a request",
    async (_name, input, message) => {
      const user = userEvent.setup();
      renderDialog();

      await user.type(screen.getByLabelText(/^label$/i), "Invalid attributes");
      fireEvent.change(screen.getByTestId("libsecret-item-attributes-input"), {
        target: { value: input },
      });
      await user.type(
        screen.getByTestId("libsecret-item-secret-input"),
        "secret",
      );
      await user.click(screen.getByRole("button", { name: /^create$/i }));

      expect(await screen.findByText(message)).toBeInTheDocument();
      expect(postLibsecretItem).not.toHaveBeenCalled();
      expect(screen.getByRole("dialog")).toBeInTheDocument();
    },
  );

  it.each(["item_1", "a".repeat(31), "a".repeat(33), "A".repeat(32), "g".repeat(32)])(
    "rejects invalid item id %s before sending a request", async (id) => {
      renderDialog();
      fireEvent.change(screen.getByLabelText(/id \(optional\)/i), { target: { value: id } });
      fireEvent.click(screen.getByRole("button", { name: /^create$/i }));
      expect(await screen.findByText("Item id must contain exactly 32 lowercase hexadecimal characters.")).toBeInTheDocument();
      expect(postLibsecretItem).not.toHaveBeenCalled();
    },
  );

  it("rejects a duplicate valid item id before sending a request", async () => {
    const id = "a".repeat(32);
    renderDialog([id]);
    fireEvent.change(screen.getByLabelText(/id \(optional\)/i), { target: { value: id } });
    fireEvent.click(screen.getByRole("button", { name: /^create$/i }));
    expect(await screen.findByText("An item with that id already exists in this collection.")).toBeInTheDocument();
    expect(postLibsecretItem).not.toHaveBeenCalled();
  });

  it("submits an explicit valid item id unchanged", async () => {
    const id = "abcdef0123456789".repeat(2);
    renderDialog();
    fireEvent.change(screen.getByLabelText(/id \(optional\)/i), { target: { value: id } });
    fireEvent.change(screen.getByLabelText(/^label$/i), { target: { value: "Item" } });
    fireEvent.change(screen.getByTestId("libsecret-item-secret-input"), { target: { value: "synthetic" } });
    fireEvent.click(screen.getByRole("button", { name: /^create$/i }));
    await waitFor(() => expect(postLibsecretItem).toHaveBeenCalled());
    expect(vi.mocked(postLibsecretItem).mock.calls[0]?.[3].id).toBe(id);
  });

  it("sends the exact parsed attribute map and Unicode secret bytes", async () => {
    const user = userEvent.setup();
    renderDialog();

    await user.type(screen.getByLabelText(/^label$/i), "Unicode secret");
    fireEvent.change(screen.getByTestId("libsecret-item-attributes-input"), {
      target: {
        value:
          " service = github.com\r\n\r\nService=internal\r\nempty=\r\nquery=alpha=beta",
      },
    });
    await user.type(
      screen.getByTestId("libsecret-item-secret-input"),
      "päss🔐",
    );
    await user.click(screen.getByRole("button", { name: /^create$/i }));

    await waitFor(() => {
      expect(postLibsecretItem).toHaveBeenCalledWith("prod", "r1", "default", {
        id: undefined,
        label: "Unicode secret",
        content_type: "text/plain",
        attributes: {
          service: "github.com",
          Service: "internal",
          empty: "",
          query: "alpha=beta",
        },
        secret_b64: "cMOkc3Pwn5SQ",
      });
    });
  });
});
