import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
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

  it("sends Unicode secret text as exact UTF-8 base64", async () => {
    const user = userEvent.setup();
    renderDialog();

    await user.type(screen.getByLabelText(/^label$/i), "Unicode secret");
    await user.type(
      screen.getByTestId("libsecret-item-secret-input"),
      "päss🔐",
    );
    await user.click(screen.getByRole("button", { name: /^create$/i }));

    expect(postLibsecretItem).toHaveBeenCalledWith("prod", "r1", "default", {
      id: undefined,
      label: "Unicode secret",
      content_type: "text/plain",
      attributes: {},
      secret_b64: "cMOkc3Pwn5SQ",
    });
  });
});
