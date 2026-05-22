import { afterEach, describe, expect, it, vi } from "vitest";

import {
  ApiError,
  _resetCsrfTokenForTesting,
  client,
  fetchWhoami,
  postLogin,
  postLogout,
  postSecretRead,
} from "./client";

function mockResponse(
  status: number,
  body: unknown,
  init?: Partial<ResponseInit>,
): Response {
  const text = typeof body === "string" ? body : JSON.stringify(body);
  return new Response(text, {
    status,
    headers: { "Content-Type": "application/json" },
    ...init,
  });
}

function mockResponseWithCsrf(
  status: number,
  body: unknown,
  csrfToken: string,
): Response {
  const text = typeof body === "string" ? body : JSON.stringify(body);
  return new Response(text, {
    status,
    headers: {
      "Content-Type": "application/json",
      "X-CSRF-Token": csrfToken,
    },
  });
}

afterEach(() => {
  vi.restoreAllMocks();
  _resetCsrfTokenForTesting();
});

describe("typed admin client", () => {
  it("sends credentials on every request", async () => {
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(mockResponse(200, { authenticated: false }));
    await fetchWhoami();
    expect(spy).toHaveBeenCalledTimes(1);
    // openapi-fetch passes the request object as a Request instance.
    const req = spy.mock.calls[0]![0] as Request;
    expect(req.credentials).toBe("include");
  });

  it("replays the cached X-CSRF-Token on POST after seeing it on a prior response", async () => {
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        mockResponseWithCsrf(200, { authenticated: false }, "auth-token"),
      )
      .mockResolvedValueOnce(mockResponse(200, { username: "alice" }));
    await fetchWhoami();
    await postLogin({ username: "alice", password: "irrelevant" });
    const req = spy.mock.calls[1]![0] as Request;
    expect(req.headers.get("X-CSRF-Token")).toBe("auth-token");
  });

  it("does not attach X-CSRF-Token on GET", async () => {
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        mockResponseWithCsrf(200, { authenticated: false }, "tok"),
      );
    await fetchWhoami();
    const req = spy.mock.calls[0]![0] as Request;
    expect(req.headers.get("X-CSRF-Token")).toBeNull();
  });

  it("does not attach X-CSRF-Token on POST when no prior response has primed the token", async () => {
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(mockResponse(200, { username: "alice" }));
    await postLogin({ username: "alice", password: "irrelevant" });
    const req = spy.mock.calls[0]![0] as Request;
    expect(req.headers.get("X-CSRF-Token")).toBeNull();
  });

  it("throws typed ApiError on non-2xx with backend code", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockResponse(401, { code: "session_revoked", message: "go away" }),
    );
    await expect(
      postSecretRead({
        store: "default",
        resource_id: "res_x",
        slot: "env",
        key: "K",
      }),
    ).rejects.toMatchObject({
      name: "ApiError",
      status: 401,
      code: "session_revoked",
      message: "go away",
    });
  });

  it("falls back to a generic code when the error body isn't JSON", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response("not json", { status: 500 }),
    );
    const err = await fetchWhoami().catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect((err as ApiError).status).toBe(500);
    expect((err as ApiError).code).toBe("error");
  });

  it("returns undefined for 204 No Content responses", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(null, { status: 204 }),
    );
    const result = await postLogout();
    expect(result).toBeUndefined();
  });

  it("does not log request or response bodies (paranoia)", async () => {
    const log = vi.spyOn(console, "log").mockImplementation(() => {});
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockResponse(200, { material_b64: "do-not-leak" }),
    );
    const result = await postSecretRead({
      store: "default",
      resource_id: "res_x",
      slot: "env",
      key: "sensitive-key",
    });
    expect(result?.material_b64).toBe("do-not-leak");
    for (const spy of [log, warn, error]) {
      for (const call of spy.mock.calls) {
        const stringified = call.map(String).join(" ");
        expect(stringified).not.toContain("do-not-leak");
        expect(stringified).not.toContain("sensitive-key");
      }
    }
  });

  it("exposes the raw openapi-fetch client for advanced usage", () => {
    expect(typeof client.GET).toBe("function");
    expect(typeof client.POST).toBe("function");
  });
});
