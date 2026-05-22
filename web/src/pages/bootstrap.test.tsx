import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { BootstrapPage } from "./bootstrap";

function mockJsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("BootstrapPage", () => {
  it("enforces password length and confirmation match", async () => {
    const user = userEvent.setup();
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    render(<BootstrapPage onSuccess={vi.fn()} />);

    await user.type(screen.getByLabelText(/^username$/i), "root");
    await user.type(screen.getByLabelText(/^password$/i), "short");
    await user.type(screen.getByLabelText(/confirm password/i), "mismatch");
    await user.click(
      screen.getByRole("button", { name: /create first admin/i }),
    );

    // Use the validation-message wording (not the always-on description),
    // which starts with "Password must be at least…" — distinct from the
    // description's "At least 12 characters." text.
    expect(
      await screen.findByText(/password must be at least 12 characters/i),
    ).toBeInTheDocument();
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("surfaces password mismatch when length is OK", async () => {
    const user = userEvent.setup();
    render(<BootstrapPage onSuccess={vi.fn()} />);

    await user.type(screen.getByLabelText(/^username$/i), "root");
    await user.type(
      screen.getByLabelText(/^password$/i),
      "longenoughpw1234",
    );
    await user.type(screen.getByLabelText(/confirm password/i), "different1234");
    await user.click(
      screen.getByRole("button", { name: /create first admin/i }),
    );

    expect(
      await screen.findByText(/passwords do not match/i),
    ).toBeInTheDocument();
  });

  it("creates the first admin and calls onSuccess", async () => {
    const user = userEvent.setup();
    const onSuccess = vi.fn();
    const fetchSpy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        mockJsonResponse(200, { username: "root", role: "admin" }),
      );
    render(<BootstrapPage onSuccess={onSuccess} />);

    await user.type(screen.getByLabelText(/^username$/i), "root");
    await user.type(
      screen.getByLabelText(/^password$/i),
      "longenoughpw1234",
    );
    await user.type(
      screen.getByLabelText(/confirm password/i),
      "longenoughpw1234",
    );
    await user.click(
      screen.getByRole("button", { name: /create first admin/i }),
    );

    await vi.waitFor(() => expect(onSuccess).toHaveBeenCalledTimes(1));
    const req = fetchSpy.mock.calls[0]![0] as Request;
    expect(req.url.endsWith("/v1/admin/auth/bootstrap")).toBe(true);
    expect(req.method).toBe("POST");
    const body = await req.text();
    expect(body).toBe(
      JSON.stringify({ username: "root", password: "longenoughpw1234" }),
    );
  });

  it("surfaces bootstrap_already_completed", async () => {
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockJsonResponse(409, {
        code: "bootstrap_already_completed",
        message: "no",
      }),
    );
    render(<BootstrapPage onSuccess={vi.fn()} />);
    await user.type(screen.getByLabelText(/^username$/i), "root");
    await user.type(
      screen.getByLabelText(/^password$/i),
      "longenoughpw1234",
    );
    await user.type(
      screen.getByLabelText(/confirm password/i),
      "longenoughpw1234",
    );
    await user.click(
      screen.getByRole("button", { name: /create first admin/i }),
    );
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(/already been completed/i);
  });

  it("uses correct new-password autocomplete on both password fields", () => {
    render(<BootstrapPage onSuccess={vi.fn()} />);
    expect(screen.getByLabelText(/^password$/i)).toHaveAttribute(
      "autocomplete",
      "new-password",
    );
    expect(screen.getByLabelText(/confirm password/i)).toHaveAttribute(
      "autocomplete",
      "new-password",
    );
  });
});
