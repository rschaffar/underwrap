import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

import type { WrapperPermission } from "@/api/client";

import { PolicyViewerDialog } from "./policy-viewer-dialog";

describe("PolicyViewerDialog", () => {
  it("renders the formatted JSON, the writable-permissions badge, and a copy button for a non-trivial policy", async () => {
    const permissions: WrapperPermission[] = [
      {
        client_instance: "dev-laptop",
        wrapper: "gh-authorized",
        resource_id: "github/token",
        mode: { kind: "unlocked" },
        writable: true,
        reason: undefined,
      },
      {
        client_instance: "ci-runner",
        wrapper: "aws-prod",
        resource_id: "aws/prod",
        mode: { kind: "locked" },
        writable: false,
        reason: "off-hours",
      },
      {
        client_instance: "ci-runner",
        wrapper: "deploy",
        resource_id: "deploy/release",
        mode: { kind: "locked" },
        writable: false,
        reason: "release window closed",
      },
    ];

    render(
      <PolicyViewerDialog
        storeId="prod"
        open
        permissions={permissions}
        onClose={() => {}}
      />,
    );

    expect(
      await screen.findByRole("heading", { name: /raw policy: prod/i }),
    ).toBeInTheDocument();

    expect(
      await screen.findByTestId("writable-permissions-badge"),
    ).toHaveTextContent(/1 of 3/i);

    const pre = await screen.findByTestId("policy-json");
    expect(pre).toHaveTextContent("gh-authorized");
    expect(pre).toHaveTextContent("aws-prod");
    expect(pre).toHaveTextContent("deploy");
    expect(pre).toHaveTextContent("off-hours");
    expect(pre.textContent).toContain("\n    ");

    expect(screen.getByRole("button", { name: /copy json/i })).toBeEnabled();
  });

  it("shows 0 writable when the policy is empty", () => {
    render(
      <PolicyViewerDialog
        storeId="dev"
        open
        permissions={[]}
        onClose={() => {}}
      />,
    );
    expect(screen.getByTestId("writable-permissions-badge")).toHaveTextContent(
      /0 of 0/i,
    );
  });

  it("copies the formatted JSON to the clipboard when the copy button is clicked", async () => {
    const permissions: WrapperPermission[] = [
      {
        client_instance: "dev-laptop",
        wrapper: "gh-authorized",
        resource_id: "github/token",
        mode: { kind: "unlocked" },
        writable: false,
        reason: undefined,
      },
    ];

    const writeText = vi.fn().mockResolvedValue(undefined);
    // jsdom doesn't expose a real Clipboard API; define a writable
    // shim so `navigator.clipboard.writeText` resolves to our spy.
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    render(
      <PolicyViewerDialog
        storeId="prod"
        open
        permissions={permissions}
        onClose={() => {}}
      />,
    );
    const copyBtn = await screen.findByRole("button", { name: /copy json/i });
    fireEvent.click(copyBtn);

    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    const arg = writeText.mock.calls[0][0] as string;
    expect(arg).toContain("gh-authorized");
    expect(arg).toContain('"client_instance":');

    expect(
      await screen.findByRole("button", { name: /copied/i }),
    ).toBeInTheDocument();
  });
});
