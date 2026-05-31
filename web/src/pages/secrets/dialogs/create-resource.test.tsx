import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { postResourceCreate } from "@/api/client";

import { CreateResourceDialog } from "./create-resource";

vi.mock("@/api/client", async () => {
  const actual = await vi.importActual<typeof import("@/api/client")>(
    "@/api/client",
  );
  return { ...actual, postResourceCreate: vi.fn() };
});

function renderDialog(existingResources: string[]) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <CreateResourceDialog
        storeId="prod"
        existingResources={existingResources}
        onClose={() => {}}
      />
    </QueryClientProvider>,
  );
}

describe("CreateResourceDialog", () => {
  beforeEach(() => {
    vi.mocked(postResourceCreate).mockReset();
    vi.mocked(postResourceCreate).mockResolvedValue(undefined as never);
  });

  it("rejects a duplicate name without hitting the network", async () => {
    const user = userEvent.setup();
    renderDialog(["github/token"]);
    await user.type(
      screen.getByTestId("create-resource-input"),
      "github/token",
    );
    await user.click(screen.getByRole("button", { name: /^create$/i }));

    // The duplicate is rejected with an inline field message (not a
    // generic "network error" Alert) and the create call is never made.
    expect(
      await screen.findByText(/a resource with that name already exists/i),
    ).toBeInTheDocument();
    expect(postResourceCreate).not.toHaveBeenCalled();
  });

  it("creates a new resource when the name is unique", async () => {
    const user = userEvent.setup();
    renderDialog(["github/token"]);
    await user.type(screen.getByTestId("create-resource-input"), "aws/prod");
    await user.click(screen.getByRole("button", { name: /^create$/i }));

    expect(postResourceCreate).toHaveBeenCalledWith("prod", {
      name: "aws/prod",
    });
  });
});
