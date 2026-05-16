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

  it("renders the formatted JSON, the writes-enabled badge, and a copy button for a non-trivial policy", async () => {
    vi.mocked(postPolicyGet).mockResolvedValue({
      document: {
        grants: [
          {
            wrapper: "gh-authorized",
            host: "deepthought",
            user: "robert",
            mode: { kind: "unlocked" },
            reason: undefined,
          },
          {
            wrapper: "aws-prod",
            host: "ci",
            user: "bot",
            mode: { kind: "locked" },
            reason: "off-hours",
          },
          {
            wrapper: "deploy",
            host: "ci",
            user: "bot",
            mode: { kind: "per-call-approval", approval_key: "ssh-ed25519 AAA…" },
            reason: undefined,
          },
        ],
        files_writable: { "github/token": true, "aws/access-key": false },
        libsecret_writable: { "github/gh": true },
      },
    });

    renderWithQuery(
      <PolicyViewerDialog
        storeName="prod"
        open={true}
        onClose={() => {}}
      />,
    );

    // The dialog opens with the store name in the title.
    expect(
      await screen.findByRole("heading", { name: /raw policy: prod/i }),
    ).toBeInTheDocument();

    // Wrapper-writes summary surfaces both per-slot counts.
    expect(await screen.findByTestId("writable-files-badge")).toHaveTextContent(
      /files: 1 on/i,
    );
    expect(screen.getByTestId("writable-libsecret-badge")).toHaveTextContent(
      /libsecret: 1 on/i,
    );

    // The JSON pre block contains every grant tuple. Asserting on
    // the wrapper names is the most legible cross-section.
    const pre = await screen.findByTestId("policy-json");
    expect(pre).toHaveTextContent("gh-authorized");
    expect(pre).toHaveTextContent("aws-prod");
    expect(pre).toHaveTextContent("deploy");
    // And on the reason for the locked one (proves we render the
    // full document, not just the keys).
    expect(pre).toHaveTextContent("off-hours");
    // Indented JSON: two-space indent before any nested grant field.
    expect(pre.textContent).toContain("\n    ");

    // Copy button is enabled.
    const copyBtn = screen.getByRole("button", { name: /copy json/i });
    expect(copyBtn).toBeEnabled();
  });

  it("shows 0-on counts when no resources have wrapper writes enabled", async () => {
    vi.mocked(postPolicyGet).mockResolvedValue({
      document: { grants: [], files_writable: {}, libsecret_writable: {} },
    });
    renderWithQuery(
      <PolicyViewerDialog storeName="dev" open={true} onClose={() => {}} />,
    );
    expect(await screen.findByTestId("writable-files-badge")).toHaveTextContent(
      /files: 0 on/i,
    );
    expect(screen.getByTestId("writable-libsecret-badge")).toHaveTextContent(
      /libsecret: 0 on/i,
    );
  });

  it("surfaces an API error in an alert without rendering the JSON", async () => {
    const { ApiError } = await import("@/api/client");
    vi.mocked(postPolicyGet).mockRejectedValue(
      new ApiError(500, "internal_error", "the backend exploded"),
    );
    renderWithQuery(
      <PolicyViewerDialog storeName="prod" open={true} onClose={() => {}} />,
    );
    // `describeApiError` falls through to the raw message for codes
    // it does not specifically translate.
    expect(await screen.findByRole("alert")).toHaveTextContent(
      /the backend exploded/i,
    );
    expect(screen.queryByTestId("policy-json")).not.toBeInTheDocument();
  });

  it("copies the formatted JSON to the clipboard when the copy button is clicked", async () => {
    vi.mocked(postPolicyGet).mockResolvedValue({
      document: {
        grants: [
          {
            wrapper: "gh-authorized",
            host: "deepthought",
            user: "robert",
            mode: { kind: "unlocked" },
            reason: undefined,
          },
        ],
        files_writable: {},
        libsecret_writable: {},
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
    // Wait for the JSON to land and the button to enable. Radix
    // Dialog renders via a portal and the button's disabled state
    // can lag a render behind the data fetch.
    await screen.findByTestId("policy-json");
    const copyBtn = screen.getByRole("button", { name: /copy json/i });
    await waitFor(() => expect(copyBtn).toBeEnabled());
    // Use fireEvent: userEvent's pointer simulation in jsdom can
    // miss buttons rendered inside Radix's portal under specific
    // focus conditions, but the React onClick still fires via a
    // plain click event.
    fireEvent.click(copyBtn);

    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    const arg = writeText.mock.calls[0][0] as string;
    expect(arg).toContain("gh-authorized");
    expect(arg).toContain('"files_writable":');
    expect(arg).toContain('"libsecret_writable":');

    // The button flips to the "Copied" affordance.
    expect(
      await screen.findByRole("button", { name: /copied/i }),
    ).toBeInTheDocument();
  });
});
