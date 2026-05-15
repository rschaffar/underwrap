import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError, apiFetch, apiJson } from "./client";

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

afterEach(() => {
  vi.restoreAllMocks();
  // Reset cookies between tests so CSRF state doesn't leak.
  document.cookie
    .split(";")
    .filter((c) => c.includes("="))
    .forEach((c) => {
      const name = c.split("=")[0]?.trim();
      if (name) document.cookie = `${name}=; Max-Age=0; Path=/`;
    });
});

describe("apiFetch", () => {
  it("sends credentials on every request", async () => {
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(mockResponse(200, { ok: true }));
    await apiFetch("/v1/admin/auth/whoami");
    expect(spy).toHaveBeenCalledTimes(1);
    const init = spy.mock.calls[0]![1]!;
    expect(init.credentials).toBe("include");
  });

  it("attaches X-CSRF-Token from the cookie on POST", async () => {
    document.cookie = "__Host-uw-csrf=token-from-cookie; Path=/; Secure";
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(mockResponse(200, { ok: true }));
    await apiFetch("/v1/admin/anything", {
      method: "POST",
      json: { a: 1 },
    });
    const headers = new Headers(spy.mock.calls[0]![1]!.headers);
    expect(headers.get("X-CSRF-Token")).toBe("token-from-cookie");
    expect(headers.get("Content-Type")).toBe("application/json");
  });

  it("does not attach X-CSRF-Token on GET", async () => {
    document.cookie = "__Host-uw-csrf=token-from-cookie; Path=/; Secure";
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(mockResponse(200, { ok: true }));
    await apiFetch("/v1/admin/auth/whoami");
    const headers = new Headers(spy.mock.calls[0]![1]!.headers);
    expect(headers.get("X-CSRF-Token")).toBeNull();
  });

  it("throws typed ApiError on non-2xx with backend code", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockResponse(401, { code: "session_revoked", message: "go away" }),
    );
    await expect(apiFetch("/v1/admin/secret/read", { method: "POST" })).rejects
      .toMatchObject({
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
    const err = await apiFetch("/v1/admin/whatever").catch((e) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err.status).toBe(500);
    expect(err.code).toBe("error");
  });

  it("does not log request or response bodies (paranoia)", async () => {
    const log = vi.spyOn(console, "log").mockImplementation(() => {});
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockResponse(200, { secret: "do-not-leak" }),
    );
    const result = await apiJson<{ secret: string }>("/v1/admin/secret/read", {
      method: "POST",
      json: { sensitive: "input" },
    });
    expect(result.secret).toBe("do-not-leak");
    for (const spy of [log, warn, error]) {
      for (const call of spy.mock.calls) {
        const stringified = call.map(String).join(" ");
        expect(stringified).not.toContain("do-not-leak");
        expect(stringified).not.toContain("sensitive");
      }
    }
  });
});
