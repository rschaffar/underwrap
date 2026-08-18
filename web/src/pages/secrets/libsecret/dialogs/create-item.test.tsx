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

function renderDialog() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <CreateItemDialog
        storeId="prod"
        resourceId="r1"
        collectionId="default"
        existingIds={[]}
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
