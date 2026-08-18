import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { postSecretRename, putSecret } from "@/api/client";

import { WriteSecretDialog } from "./write";
import type { SecretAddress } from "../grouping";

vi.mock("@/api/client", async () => {
  const actual = await vi.importActual<typeof import("@/api/client")>(
    "@/api/client",
  );
  return { ...actual, putSecret: vi.fn(), postSecretRename: vi.fn() };
});

function renderDialog(mode: "create" | "edit", address: SecretAddress) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <WriteSecretDialog
        storeId="prod"
        mode={mode}
        address={address}
        onClose={() => {}}
      />
    </QueryClientProvider>,
  );
}

describe("WriteSecretDialog", () => {
  beforeEach(() => {
    vi.mocked(putSecret).mockReset();
    vi.mocked(putSecret).mockResolvedValue(undefined as never);
    vi.mocked(postSecretRename).mockReset();
    vi.mocked(postSecretRename).mockResolvedValue(undefined as never);
  });

  it("writes a new entry via putSecret with base64-encoded material", async () => {
    const user = userEvent.setup();
    renderDialog("create", { resource_id: "r1", slot: "env", key: "" });
    await user.type(screen.getByLabelText(/^key$/i), "GH_TOKEN");
    await user.type(screen.getByTestId("material-input"), "hello");
    await user.click(screen.getByRole("button", { name: /^create$/i }));

    expect(putSecret).toHaveBeenCalledWith("prod", "r1", "env", "GH_TOKEN", {
      material_b64: btoa("hello"),
    });
    expect(postSecretRename).not.toHaveBeenCalled();
  });

  it("writes Unicode material as its exact UTF-8 base64 body", async () => {
    const user = userEvent.setup();
    renderDialog("create", { resource_id: "r1", slot: "files", key: "" });
    await user.type(screen.getByLabelText(/^key$/i), "token.txt");
    await user.type(screen.getByTestId("material-input"), "✓ à la mode");
    await user.click(screen.getByRole("button", { name: /^create$/i }));

    expect(putSecret).toHaveBeenCalledWith(
      "prod",
      "r1",
      "files",
      "token.txt",
      { material_b64: "4pyTIMOgIGxhIG1vZGU=" },
    );
  });

  it("renames via postSecretRename when the key changes in edit mode", async () => {
    const user = userEvent.setup();
    renderDialog("edit", { resource_id: "r1", slot: "env", key: "OLD" });
    const keyField = screen.getByLabelText(/^key$/i);
    await user.clear(keyField);
    await user.type(keyField, "NEW");
    await user.type(screen.getByTestId("material-input"), "hi");
    await user.click(screen.getByRole("button", { name: /^save$/i }));

    expect(postSecretRename).toHaveBeenCalledWith("prod", "r1", "env", "OLD", {
      to_key: "NEW",
      material_b64: btoa("hi"),
    });
    expect(putSecret).not.toHaveBeenCalled();
  });

  it("renames with Unicode replacement material as exact UTF-8 base64", async () => {
    const user = userEvent.setup();
    renderDialog("edit", { resource_id: "r1", slot: "env", key: "OLD" });
    const keyField = screen.getByLabelText(/^key$/i);
    await user.clear(keyField);
    await user.type(keyField, "NEW");
    await user.type(screen.getByTestId("material-input"), "päss🔐");
    await user.click(screen.getByRole("button", { name: /^save$/i }));

    expect(postSecretRename).toHaveBeenCalledWith("prod", "r1", "env", "OLD", {
      to_key: "NEW",
      material_b64: "cMOkc3Pwn5SQ",
    });
  });
});
