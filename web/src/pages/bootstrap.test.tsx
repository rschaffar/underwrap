import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

import { BootstrapPage } from "./bootstrap";

describe("BootstrapPage", () => {
  it("shows actionable offline provisioning guidance", () => {
    render(<BootstrapPage />);

    expect(
      screen.getByRole("heading", { name: /first-time setup required/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/stop the underwrap backend service/i)).toBeVisible();
    expect(screen.getByText(/bootstrap-user --username/i)).toBeVisible();
    expect(screen.getByRole("link", { name: /continue to sign in/i })).toHaveAttribute(
      "href",
      "/login",
    );
  });

  it("contains no credential form or network bootstrap action", () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    render(<BootstrapPage />);

    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/password/i)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /create/i })).not.toBeInTheDocument();
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
