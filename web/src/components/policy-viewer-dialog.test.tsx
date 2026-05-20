import { describe, expect, it, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { PolicyViewerDialog } from "./policy-viewer-dialog";

// `postPolicyGet` is the only API call this dialog makes. Mock it
// directly so the test stays hermetic.
vi.mock("@/api/client", async () => {
  const actual =
    await vi.importActual<typeof import("@/api/client")>("@/api/client");
  return {
    ...actual,
    postPolicyGet: vi.fn(),
  };
});

import { postPolicyGet } from "@/api/client";

function renderWithQuery(ui: React.ReactNode) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(<QueryClientProvider client={qc}>{ui}</QueryClientProvider>);
}

describe("PolicyViewerDialog", () => {
  beforeEach(() => {
    vi.mocked(postPolicyGet).mockReset();
  });

  it("renders the formatted JSON, the writable-permissions badge, and a copy button for a non-trivial policy", async () => {
    vi.mocked(postPolicyGet).mockResolvedValue({
      document: {
        permissions: [
          {
            client_instance: "dev-laptop",
            wrapper: "gh-authorized",
            resource: "github/token",
            mode: { kind: "unlocked" },
            writable: true,
            reason: undefined,
          },
          {
            client_instance: "ci-runner",
            wrapper: "aws-prod",
            resource: "aws/prod",
            mode: { kind: "locked" },
            writable: false,
            reason: "off-hours",
          },
          {
            client_instance: "ci-runner",
            wrapper: "deploy",
            resource: "deploy/release",
            mode: { kind: "per-call-approval", approval_key: "ssh-ed25519 AAA…" },
            writable: false,
            reason: undefined,
          },
        ],
      },
    });

    renderWithQuery(
      <PolicyViewerDialog storeName="prod" open={true} onClose={() => {}} />,
    );

    // The dialog opens with the store name in the title.
    expect(
      await screen.findByRole("heading", { name: /raw policy: prod/i }),
    ).toBeInTheDocument();

    // Writable-permissions badge counts the single permission flagged writable.
    expect(
      await screen.findByTestId("writable-permissions-badge"),
    ).toHaveTextContent(/1 of 3/i);

    // The JSON pre block contains every permission tuple.
    const pre = await screen.findByTestId("policy-json");
    expect(pre).toHaveTextContent("gh-authorized");
    expect(pre).toHaveTextContent("aws-prod");
    expect(pre).toHaveTextContent("deploy");
    expect(pre).toHaveTextContent("off-hours");
    expect(pre.textContent).toContain("\n    ");

    const copyBtn = screen.getByRole("button", { name: /copy json/i });
    expect(copyBtn).toBeEnabled();
  });

  it("shows 0 writable when the policy is empty", async () => {
    vi.mocked(postPolicyGet).mockResolvedValue({
      document: { permissions: [] },
    });
    renderWithQuery(
      <PolicyViewerDialog storeName="dev" open={true} onClose={() => {}} />,
    );
    expect(
      await screen.findByTestId("writable-permissions-badge"),
    ).toHaveTextContent(/0 of 0/i);
  });

  it("surfaces an API error in an alert without rendering the JSON", async () => {
    const { ApiError } = await import("@/api/client");
    vi.mocked(postPolicyGet).mockRejectedValue(
      new ApiError(500, "internal_error", "the backend exploded"),
    );
    renderWithQuery(
      <PolicyViewerDialog storeName="prod" open={true} onClose={() => {}} />,
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      /the backend exploded/i,
    );
    expect(screen.queryByTestId("policy-json")).not.toBeInTheDocument();
  });

  it("copies the formatted JSON to the clipboard when the copy button is clicked", async () => {
    vi.mocked(postPolicyGet).mockResolvedValue({
      document: {
        permissions: [
          {
            client_instance: "dev-laptop",
            wrapper: "gh-authorized",
            resource: "github/token",
            mode: { kind: "unlocked" },
            writable: false,
            reason: undefined,
          },
        ],
      },
    });
    const writeText = vi.fn().mockResolvedValue(undefined);
    // jsdom doesn't expose a real Clipboard API; define a writable
    // shim so `navigator.clipboard.writeText` resolves to our spy.
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    renderWithQuery(
      <PolicyViewerDialog storeName="prod" open={true} onClose={() => {}} />,
    );
    await screen.findByTestId("policy-json");
    const copyBtn = screen.getByRole("button", { name: /copy json/i });
    await waitFor(() => expect(copyBtn).toBeEnabled());
    fireEvent.click(copyBtn);

    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    const arg = writeText.mock.calls[0][0] as string;
    expect(arg).toContain("gh-authorized");
    expect(arg).toContain('"client_instance":');

    // The button flips to the "Copied" affordance.
    expect(
      await screen.findByRole("button", { name: /copied/i }),
    ).toBeInTheDocument();
  });
});
