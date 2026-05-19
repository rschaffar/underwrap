import { afterEach, describe, expect, it, vi } from "vitest";
import {
  ApiError,
  apiFetch,
  apiJson,
  fetchBootstrapStatus,
  fetchWhoami,
  postBootstrap,
  postLogin,
  postLogout,
  postPairCreate,
  postPairDelete,
  postPairsList,
  postResourceCreate,
  postResourceDelete,
  postResourcesList,
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

  it("forwards rawBody verbatim and does not stringify it", async () => {
    const spy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(mockResponse(200, { ok: true }));
    const blob = new Blob(["raw bytes"]);
    await apiFetch("/v1/admin/upload", { method: "POST", rawBody: blob });
    const init = spy.mock.calls[0]![1]!;
    expect(init.body).toBe(blob);
    // Content-Type is NOT auto-set for rawBody — the caller owns it.
    expect(new Headers(init.headers).get("Content-Type")).toBeNull();
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

describe("auth endpoint helpers", () => {
  it("routes each helper to the documented admin path", async () => {
    const calls: Array<[string, string]> = [];
    vi.spyOn(globalThis, "fetch").mockImplementation(
      async (input: RequestInfo | URL, init?: RequestInit) => {
        calls.push([String(input), init?.method ?? "GET"]);
        if (String(input).endsWith("/auth/bootstrap/status")) {
          return mockResponse(200, { bootstrap_required: false });
        }
        if (String(input).endsWith("/auth/whoami")) {
          return mockResponse(200, { authenticated: false });
        }
        // 204 → apiJson returns undefined; covers postLogout.
        return new Response(null, { status: 204 });
      },
    );

    await fetchWhoami();
    await fetchBootstrapStatus();
    await postLogin({ username: "u", password: "p" });
    await postLogout();
    await postBootstrap({ username: "u", password: "longenoughpw1234" });

    expect(calls).toEqual([
      ["/v1/admin/auth/whoami", "GET"],
      ["/v1/admin/auth/bootstrap/status", "GET"],
      ["/v1/admin/auth/login", "POST"],
      ["/v1/admin/auth/logout", "POST"],
      ["/v1/admin/auth/bootstrap", "POST"],
    ]);
  });
});

describe("resources and pairs admin helpers", () => {
  it("each helper hits the documented POST route with the request body", async () => {
    const calls: Array<{ path: string; method: string; body: unknown }> = [];
    vi.spyOn(globalThis, "fetch").mockImplementation(
      async (input: RequestInfo | URL, init?: RequestInit) => {
        const body = init?.body
          ? JSON.parse(init.body as string)
          : undefined;
        calls.push({
          path: String(input),
          method: init?.method ?? "GET",
          body,
        });
        // All responses follow the same shape: stub with the empty-list /
        // ok shapes; tests below only verify request wiring, not response
        // contents.
        if (String(input).endsWith("/resources/list")) {
          return mockResponse(200, { resources: [] });
        }
        if (String(input).endsWith("/pairs/list")) {
          return mockResponse(200, { pairs: [] });
        }
        if (String(input).endsWith("/resources/create")) {
          return mockResponse(200, {
            resource: {
              name: "r",
              created_by: { kind: "operator", id: "op", username: "a" },
              created_at: 1,
            },
          });
        }
        if (String(input).endsWith("/pairs/create")) {
          return mockResponse(200, {
            pair: {
              wrapper: "gh",
              resource: "r",
              created_by: { kind: "operator", id: "op", username: "a" },
              created_at: 1,
            },
          });
        }
        return mockResponse(200, { ok: true });
      },
    );

    await postResourcesList({ store: "work" });
    await postResourceCreate({ store: "work", name: "github/token" });
    await postResourceDelete({ store: "work", name: "github/token" });
    await postPairsList({ store: "work" });
    await postPairCreate({ store: "work", wrapper: "gh", resource: "r" });
    await postPairDelete({ store: "work", wrapper: "gh", resource: "r" });

    expect(calls.map((c) => [c.path, c.method])).toEqual([
      ["/v1/admin/resources/list", "POST"],
      ["/v1/admin/resources/create", "POST"],
      ["/v1/admin/resources/delete", "POST"],
      ["/v1/admin/pairs/list", "POST"],
      ["/v1/admin/pairs/create", "POST"],
      ["/v1/admin/pairs/delete", "POST"],
    ]);
    expect(calls[0]!.body).toEqual({ store: "work" });
    expect(calls[1]!.body).toEqual({ store: "work", name: "github/token" });
    expect(calls[4]!.body).toEqual({
      store: "work",
      wrapper: "gh",
      resource: "r",
    });
  });
});
