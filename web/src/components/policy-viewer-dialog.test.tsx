import { afterEach, describe, expect, it, vi } from "vitest";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";

import type { WrapperPermission } from "@/api/client";

import { PolicyViewerDialog } from "./policy-viewer-dialog";

afterEach(() => vi.restoreAllMocks());

function clipboard(writeText: () => Promise<void>) {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
}

function renderEmptyPolicy() {
  return render(
    <PolicyViewerDialog
      storeId="test"
      open
      permissions={[]}
      onClose={() => {}}
    />,
  );
}

describe("PolicyViewerDialog", () => {
  it("restarts copy feedback and cancels its owned timer on unmount", async () => {
    clipboard(vi.fn().mockResolvedValue(undefined));
    const timers = vi.spyOn(globalThis, "setTimeout");
    const clear = vi.spyOn(globalThis, "clearTimeout");
    const view = renderEmptyPolicy();
    const feedbackTimer = () => {
      const index = timers.mock.calls.findLastIndex((call) => call[1] === 1500);
      expect(index).toBeGreaterThanOrEqual(0);
      return timers.mock.results[index].value;
    };
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Copy JSON" })),
    );
    const first = feedbackTimer();
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Copied" })),
    );
    const second = feedbackTimer();
    expect(second).not.toBe(first);
    expect(clear).toHaveBeenCalledWith(first);
    view.unmount();
    expect(clear).toHaveBeenCalledWith(second);
  });

  it("resets the copied badge when the feedback deadline elapses", async () => {
    vi.useFakeTimers();
    const view = renderEmptyPolicy();
    try {
      clipboard(vi.fn().mockResolvedValue(undefined));
      await act(async () =>
        fireEvent.click(screen.getByRole("button", { name: "Copy JSON" })),
      );
      await act(() => vi.advanceTimersByTimeAsync(1499));
      expect(
        screen.getByRole("button", { name: "Copied" }),
      ).toBeInTheDocument();
      await act(() => vi.advanceTimersByTimeAsync(1));
      expect(
        screen.getByRole("button", { name: "Copy JSON" }),
      ).toBeInTheDocument();
    } finally {
      view.unmount();
      vi.useRealTimers();
    }
  });

  it("does not publish feedback or create a timer after a late clipboard completion", async () => {
    let finish!: () => void;
    clipboard(
      vi.fn(
        () =>
          new Promise<void>((resolve) => {
            finish = resolve;
          }),
      ),
    );
    const timers = vi.spyOn(globalThis, "setTimeout");
    const view = renderEmptyPolicy();
    fireEvent.click(screen.getByRole("button", { name: "Copy JSON" }));
    view.unmount();
    await act(async () => finish());
    expect(timers.mock.calls.filter((call) => call[1] === 1500)).toHaveLength(
      0,
    );
  });
  it("renders the formatted JSON and a copy button for a non-trivial policy", async () => {
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

    const pre = await screen.findByTestId("policy-json");
    expect(pre).toHaveTextContent("gh-authorized");
    expect(pre).toHaveTextContent("aws-prod");
    expect(pre).toHaveTextContent("deploy");
    expect(pre).toHaveTextContent("off-hours");
    expect(pre.textContent).toContain("\n    ");

    expect(screen.getByRole("button", { name: /copy json/i })).toBeEnabled();
  });

  it("renders an empty permissions JSON when the policy is empty", () => {
    render(
      <PolicyViewerDialog
        storeId="dev"
        open
        permissions={[]}
        onClose={() => {}}
      />,
    );
    expect(screen.getByTestId("policy-json").textContent).toContain(
      '"permissions": []',
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
