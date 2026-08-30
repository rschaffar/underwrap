import { describe, expect, it, beforeEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode, AnchorHTMLAttributes } from "react";

// The shell renders `<Link>` from TanStack Router, which requires a
// RouterProvider context. The shell's behaviour under test is its
// chrome (header, drawer, theme toggle), not link navigation, so we
// stub `Link` with a plain anchor and skip the router setup.
vi.mock("@tanstack/react-router", () => ({
  Link: ({
    to,
    children,
    ...rest
  }: { to: string; children: ReactNode } & AnchorHTMLAttributes<HTMLAnchorElement>) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));

import { AppShell } from "./app-shell";

describe("AppShell", () => {
  beforeEach(() => {
    window.localStorage.clear();
    document.documentElement.classList.remove("dark");
    document.documentElement.classList.add("dark");
  });

  it("renders a header and main region", () => {
    render(
      <AppShell>
        <div>page</div>
      </AppShell>,
    );
    expect(screen.getByRole("banner")).toBeInTheDocument();
    expect(screen.getByRole("main")).toBeInTheDocument();
    expect(screen.getByText("page")).toBeInTheDocument();
  });

  it("opens the drawer when the menu button is clicked", async () => {
    const user = userEvent.setup();
    render(
      <AppShell>
        <div />
      </AppShell>,
    );
    const trigger = screen.getByRole("button", { name: /open navigation/i });
    await user.click(trigger);
    // The drawer's title is announced via Radix's Dialog title role.
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toBeInTheDocument();
    // Closing via the close button should remove the dialog.
    const closeButton = screen.getByRole("button", { name: "Close" });
    expect(closeButton).toHaveClass(
      "focus-visible:outline-hidden",
      "focus-visible:ring-2",
      "focus-visible:ring-(--color-accent)/40",
    );
    expect(closeButton).not.toHaveClass("focus:outline-hidden");
    await user.click(closeButton);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("toggles the dark theme", async () => {
    const user = userEvent.setup();
    render(
      <AppShell>
        <div />
      </AppShell>,
    );
    // Default starts dark.
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    const toggle = screen.getByRole("button", {
      name: /switch to light theme/i,
    });
    await user.click(toggle);
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    expect(window.localStorage.getItem("uw.theme")).toBe("light");
    // Toggling again restores dark.
    const restore = screen.getByRole("button", {
      name: /switch to dark theme/i,
    });
    await user.click(restore);
    expect(document.documentElement.classList.contains("dark")).toBe(true);
  });
});
