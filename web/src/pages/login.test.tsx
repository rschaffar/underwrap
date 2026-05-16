import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { LoginPage } from "./login";
import { ApiError } from "@/api/client";

function mockJsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("LoginPage", () => {
  it("requires username and password before submitting", async () => {
    const user = userEvent.setup();
    const onSuccess = vi.fn();
    const fetchSpy = vi.spyOn(globalThis, "fetch");

    render(<LoginPage onSuccess={onSuccess} />);
    await user.click(screen.getByRole("button", { name: /sign in/i }));

    expect(await screen.findByText(/username is required/i)).toBeInTheDocument();
    expect(screen.getByText(/password is required/i)).toBeInTheDocument();
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(onSuccess).not.toHaveBeenCalled();
  });

  it("submits credentials and calls onSuccess on 200", async () => {
    const user = userEvent.setup();
    const onSuccess = vi.fn();
    const fetchSpy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        mockJsonResponse(200, { username: "alice", role: "admin" }),
      );

    render(<LoginPage onSuccess={onSuccess} />);
    await user.type(screen.getByLabelText(/username/i), "alice");
    await user.type(screen.getByLabelText(/password/i), "hunter2");
    await user.click(screen.getByRole("button", { name: /sign in/i }));

    await vi.waitFor(() => expect(onSuccess).toHaveBeenCalledTimes(1));
    expect(fetchSpy).toHaveBeenCalledTimes(1);
    const [path, init] = fetchSpy.mock.calls[0]!;
    expect(path).toBe("/v1/admin/auth/login");
    expect(init?.method).toBe("POST");
    // The body must contain what the user typed and nothing else.
    expect(init?.body).toBe(
      JSON.stringify({ username: "alice", password: "hunter2" }),
    );
  });

  it("shows an inline message on invalid_credentials and clears it on retry", async () => {
    const user = userEvent.setup();
    const onSuccess = vi.fn();
    const fetchSpy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        mockJsonResponse(401, {
          code: "invalid_credentials",
          message: "nope",
        }),
      )
      .mockResolvedValueOnce(
        mockJsonResponse(200, { username: "alice", role: "admin" }),
      );

    render(<LoginPage onSuccess={onSuccess} />);
    await user.type(screen.getByLabelText(/username/i), "alice");
    await user.type(screen.getByLabelText(/password/i), "wrong");
    await user.click(screen.getByRole("button", { name: /sign in/i }));

    expect(
      await screen.findByText(/invalid username or password/i),
    ).toBeInTheDocument();
    expect(onSuccess).not.toHaveBeenCalled();

    // Typing into the password field clears the server-set error.
    await user.clear(screen.getByLabelText(/password/i));
    await user.type(screen.getByLabelText(/password/i), "correct");
    await user.click(screen.getByRole("button", { name: /sign in/i }));
    await vi.waitFor(() => expect(onSuccess).toHaveBeenCalledTimes(1));
    expect(fetchSpy).toHaveBeenCalledTimes(2);
  });

  it("shows a rate-limit toast-style message on 429", async () => {
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockJsonResponse(429, { code: "rate_limited", message: "slow down" }),
    );
    render(<LoginPage onSuccess={vi.fn()} />);
    await user.type(screen.getByLabelText(/username/i), "alice");
    await user.type(screen.getByLabelText(/password/i), "any");
    await user.click(screen.getByRole("button", { name: /sign in/i }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(/too many attempts/i);
  });

  it("does not log the password (paranoia)", async () => {
    const user = userEvent.setup();
    const log = vi.spyOn(console, "log").mockImplementation(() => {});
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockJsonResponse(200, { username: "alice", role: "admin" }),
    );
    render(<LoginPage onSuccess={vi.fn()} />);
    await user.type(screen.getByLabelText(/username/i), "alice");
    await user.type(screen.getByLabelText(/password/i), "topsecret-1234");
    await user.click(screen.getByRole("button", { name: /sign in/i }));

    for (const spy of [log, warn, error]) {
      for (const call of spy.mock.calls) {
        expect(call.map(String).join(" ")).not.toContain("topsecret-1234");
      }
    }
  });

  it("uses correct password autocomplete hint", () => {
    render(<LoginPage onSuccess={vi.fn()} />);
    expect(screen.getByLabelText(/password/i)).toHaveAttribute(
      "autocomplete",
      "current-password",
    );
    expect(screen.getByLabelText(/username/i)).toHaveAttribute(
      "autocomplete",
      "username",
    );
  });
});

describe("ApiError mapping", () => {
  it("surfaces ApiError type so callers can switch", () => {
    expect(new ApiError(401, "invalid_credentials", "x")).toBeInstanceOf(
      ApiError,
    );
  });
});
