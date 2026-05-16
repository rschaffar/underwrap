/**
 * Typed fetch wrapper for the admin API.
 *
 * - Always sends cookies (`credentials: "include"`) so the session
 *   and CSRF cookies set by the backend are visible on every request.
 * - Reads `__Host-uw-csrf` from `document.cookie` and attaches it as
 *   `X-CSRF-Token` on any non-GET request — the double-submit
 *   half-of the CSRF check (the backend also verifies against the
 *   session's csrf_secret).
 * - Surfaces backend errors as typed [`ApiError`] objects carrying the
 *   stable `code` (`csrf_required`, `bad_origin`, `session_revoked`,
 *   etc.) so callers can switch on it without parsing strings.
 * - Never logs request or response bodies. Secret material flows
 *   through this client; accidental leaks via console.log would be a
 *   regression we audit for.
 */

const CSRF_COOKIE_NAME = "__Host-uw-csrf";
const CSRF_HEADER_NAME = "X-CSRF-Token";

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
  }
}

export interface RequestOptions extends Omit<RequestInit, "body"> {
  /**
   * JSON body. Will be `JSON.stringify`-ed and sent with
   * `Content-Type: application/json`. Mutually exclusive with
   * `rawBody`.
   */
  json?: unknown;
  /** Raw body bytes; the caller is responsible for headers. */
  rawBody?: BodyInit | null;
}

const SAFE_METHODS = new Set(["GET", "HEAD", "OPTIONS"]);

function readCookie(name: string): string | null {
  if (typeof document === "undefined") return null;
  const all = document.cookie.split(";");
  for (const part of all) {
    const trimmed = part.trim();
    const eq = trimmed.indexOf("=");
    if (eq < 0) continue;
    if (trimmed.slice(0, eq) === name) {
      return trimmed.slice(eq + 1);
    }
  }
  return null;
}

async function parseError(res: Response): Promise<ApiError> {
  // The backend's error payload is `{ code, message }`. Bodies that
  // don't parse get a generic mapping so the caller still gets a
  // typed throw.
  try {
    const data = (await res.json()) as { code?: string; message?: string };
    return new ApiError(
      res.status,
      data.code ?? "error",
      data.message ?? res.statusText,
    );
  } catch {
    return new ApiError(res.status, "error", res.statusText);
  }
}

export async function apiFetch(
  path: string,
  options: RequestOptions = {},
): Promise<Response> {
  const { json, rawBody, headers, ...rest } = options;
  const method = (rest.method ?? "GET").toUpperCase();
  const finalHeaders = new Headers(headers);
  let body: BodyInit | null = null;
  if (json !== undefined) {
    finalHeaders.set("Content-Type", "application/json");
    body = JSON.stringify(json);
  } else if (rawBody !== undefined) {
    body = rawBody;
  }
  if (!SAFE_METHODS.has(method)) {
    const token = readCookie(CSRF_COOKIE_NAME);
    if (token !== null) {
      finalHeaders.set(CSRF_HEADER_NAME, token);
    }
  }
  const res = await fetch(path, {
    ...rest,
    method,
    headers: finalHeaders,
    credentials: "include",
    body,
  });
  if (!res.ok) {
    throw await parseError(res);
  }
  return res;
}

export async function apiJson<T>(
  path: string,
  options: RequestOptions = {},
): Promise<T> {
  const res = await apiFetch(path, options);
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

// ----- Typed admin-API helpers -----
//
// All request/response shapes come from `@/api/types`, the barrel for the
// TypeScript bindings ts-rs generates from the Rust DTOs in
// `crates/shared-protocol` and `crates/backend-backendd`. Editing those
// files by hand will fail `cargo test --test ts_export`; regenerate by
// running `cargo test --workspace` instead.

import type {
  BootstrapRequest,
  BootstrapResponse,
  BootstrapStatusResponse,
  LoginRequest,
  LoginResponse,
  WhoamiResponse,
} from "@/api/types";

export type {
  BootstrapRequest,
  BootstrapResponse,
  BootstrapStatusResponse,
  LoginRequest,
  LoginResponse,
  WhoamiResponse,
};

export function fetchWhoami(): Promise<WhoamiResponse> {
  return apiJson<WhoamiResponse>("/v1/admin/auth/whoami");
}

export function fetchBootstrapStatus(): Promise<BootstrapStatusResponse> {
  return apiJson<BootstrapStatusResponse>("/v1/admin/auth/bootstrap/status");
}

export function postLogin(body: LoginRequest): Promise<LoginResponse> {
  return apiJson<LoginResponse>("/v1/admin/auth/login", {
    method: "POST",
    json: body,
  });
}

export function postLogout(): Promise<void> {
  return apiJson<void>("/v1/admin/auth/logout", { method: "POST" });
}

export function postBootstrap(body: BootstrapRequest): Promise<BootstrapResponse> {
  return apiJson<BootstrapResponse>("/v1/admin/auth/bootstrap", {
    method: "POST",
    json: body,
  });
}
