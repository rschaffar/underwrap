import { describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ConfirmDangerDialog } from "./confirm-danger-dialog";

describe("ConfirmDangerDialog", () => {
  it("renders title, description, and confirm label", async () => {
    render(
      <ConfirmDangerDialog
        title="Enable unsafe debug?"
        description={<>This prints secret material.</>}
        confirmLabel="Yes, enable"
        onConfirm={() => {}}
        onCancel={() => {}}
      />,
    );
    expect(
      await screen.findByText(/enable unsafe debug\?/i),
    ).toBeInTheDocument();
    expect(screen.getByText(/prints secret material/i)).toBeInTheDocument();
    expect(screen.getByTestId("confirm-danger-confirm")).toHaveTextContent(
      /yes, enable/i,
    );
  });

  it("focuses the cancel button by default so a stray Enter does not arm the dangerous action", async () => {
    render(
      <ConfirmDangerDialog
        title="t"
        description="d"
        confirmLabel="Yes"
        onConfirm={() => {}}
        onCancel={() => {}}
      />,
    );
    await waitFor(() =>
      expect(screen.getByTestId("confirm-danger-cancel")).toHaveFocus(),
    );
  });

  it("invokes onConfirm and onCancel from the respective buttons", async () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <ConfirmDangerDialog
        title="t"
        description="d"
        confirmLabel="Yes"
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );
    await userEvent.click(screen.getByTestId("confirm-danger-confirm"));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByTestId("confirm-danger-cancel"));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});
