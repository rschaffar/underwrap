import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ConfirmDialog } from "./confirm-dialog";

function renderDialog(overrides: Partial<React.ComponentProps<typeof ConfirmDialog>> = {}) {
  const props = {
    title: "Delete store",
    description: "This cannot be undone.",
    confirmLabel: "Delete",
    pendingLabel: "Deleting…",
    onConfirm: vi.fn(),
    onClose: vi.fn(),
    pending: false,
    submitError: null as string | null,
    ...overrides,
  };
  render(<ConfirmDialog {...props} />);
  return props;
}

describe("ConfirmDialog", () => {
  it("renders the title, description, and body", async () => {
    renderDialog({ body: <p>extra context</p> });
    expect(await screen.findByText(/delete store/i)).toBeInTheDocument();
    expect(screen.getByText(/cannot be undone/i)).toBeInTheDocument();
    expect(screen.getByText(/extra context/i)).toBeInTheDocument();
  });

  it("shows the pending label and disables confirm while pending", () => {
    renderDialog({ pending: true });
    const confirm = screen.getByRole("button", { name: /deleting…/i });
    expect(confirm).toBeDisabled();
  });

  it("disables confirm when `disabled` is set", () => {
    renderDialog({ disabled: true });
    expect(screen.getByRole("button", { name: /^delete$/i })).toBeDisabled();
  });

  it("renders a submit error as an alert", () => {
    renderDialog({ submitError: "Backend said no." });
    expect(screen.getByText(/backend said no\./i)).toBeInTheDocument();
  });

  it("fires onConfirm and onClose from the respective buttons", async () => {
    const user = userEvent.setup();
    const props = renderDialog();
    await user.click(screen.getByRole("button", { name: /^delete$/i }));
    expect(props.onConfirm).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: /cancel/i }));
    expect(props.onClose).toHaveBeenCalledTimes(1);

    const closeButton = screen.getByRole("button", { name: "Close" });
    expect(closeButton).toHaveClass(
      "focus-visible:outline-hidden",
      "focus-visible:ring-2",
      "focus-visible:ring-(--color-accent)/40",
    );
    expect(closeButton).not.toHaveClass("focus:outline-hidden");
    await user.click(closeButton);
    expect(props.onClose).toHaveBeenCalledTimes(2);
  });
});
