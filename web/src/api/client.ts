/**
 * Typed admin-API client.
 *
 * Built on top of `openapi-fetch` against the schema generated from
 * `crates/backend-backendd/openapi.json` (see `npm run generate:api`).
 * The schema is the source of truth — every request body, response
 * shape, and path is inferred from it. Hand-edits to the typed call
 * surface go through the Rust handler annotations, not this file.
 *
 * Cross-cutting behaviour layered on top:
 *
 * - `credentials: "include"` so the `__Host-uw-session` and
 *   `__Host-uw-csrf` cookies set by the backend are visible on every
 *   request.
 * - `X-CSRF-Token` synchronizer-token capture+replay. The token is
 *   served by `axum_csrf` on every admin response's `X-CSRF-Token`
 *   header; we cache it in module scope and replay it on the next
 *   non-GET/HEAD/OPTIONS request. The CSRF cookie itself is
 *   `HttpOnly` — JS never reads it.
 * - Non-2xx responses are converted into typed [`ApiError`] objects
 *   carrying the stable `code` field so callers can `switch` on it
 *   without parsing strings.
 * - No request or response bodies are ever logged. Secret material
 *   flows through this client; accidental leaks via `console.log`
 *   would be a security regression.
 */

import createClient, { type Middleware } from "openapi-fetch";

import type { components, paths } from "./schema";

const CSRF_HEADER_NAME = "X-CSRF-Token";
const SAFE_METHODS = new Set(["GET", "HEAD", "OPTIONS"]);

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

let csrfToken: string | null = null;

/** Exposed for tests only — resets the cached CSRF token. */
export function _resetCsrfTokenForTesting(): void {
  csrfToken = null;
}

const csrfMiddleware: Middleware = {
  async onRequest({ request }) {
    if (!SAFE_METHODS.has(request.method) && csrfToken !== null) {
      request.headers.set(CSRF_HEADER_NAME, csrfToken);
    }
    return request;
  },
  async onResponse({ response }) {
    const fresh = response.headers.get(CSRF_HEADER_NAME);
    if (fresh !== null && fresh !== "") {
      csrfToken = fresh;
    }
    return response;
  },
};

// `openapi-fetch` resolves request URLs through `new URL(path, base)`,
// which rejects bare paths. Anchor the client at the current page
// origin in the browser (and in jsdom under test, where the document
// URL is set in `vite.config.ts`). On a fresh SSR pass we leave it
// empty — there is no such pass in this SPA today.
const apiBaseUrl =
  typeof window !== "undefined" ? window.location.origin : "";

export const client = createClient<paths>({
  baseUrl: apiBaseUrl,
  credentials: "include",
  // Resolve `globalThis.fetch` at call time, not at module-load time.
  // openapi-fetch captures its `fetch` option in `createClient` by
  // default, which makes `vi.spyOn(globalThis, "fetch")` in tests a
  // no-op (the client still calls the original implementation). This
  // shim closes that gap with no production cost.
  fetch: (input: Request) => globalThis.fetch(input),
});
client.use(csrfMiddleware);

export async function unwrap<T>(
  call: Promise<{ data?: T; error?: unknown; response: Response }>,
): Promise<T> {
  const { data, error, response } = await call;
  if (response.ok) {
    return (data ?? (undefined as unknown)) as T;
  }
  const envelope = (error ?? {}) as { code?: string; message?: string };
  throw new ApiError(
    response.status,
    envelope.code ?? "error",
    envelope.message ?? response.statusText,
  );
}

// ----- Named schema types for callsite ergonomics -----
//
// Re-exported under their schema names so callsites can write
// `import type { LoginRequest } from "@/api/client"` the same way they
// used to. The shapes come from openapi-typescript's `components`
// table, which mirrors the Rust DTOs 1:1 via utoipa::ToSchema.
type S = components["schemas"];
export type AdminClientInstanceCreateRequest = S["AdminClientInstanceCreateRequest"];
export type AdminClientInstanceListItem = S["AdminClientInstanceListItem"];
export type AdminClientInstanceUpdateRequest = S["AdminClientInstanceUpdateRequest"];
export type AdminClientInstanceStatus = S["AdminClientInstanceStatus"];
export type AdminClientInstancesListResponse = S["AdminClientInstancesListResponse"];
export type AdminOkResponse = S["AdminOkResponse"];
export type AdminOkUserResponse = S["AdminOkUserResponse"];
export type AdminPairCreateRequest = S["AdminPairCreateRequest"];
export type AdminPairCreateResponse = S["AdminPairCreateResponse"];
export type AdminPairDeleteRequest = S["AdminPairDeleteRequest"];
export type AdminPairDeleteResponse = S["AdminPairDeleteResponse"];
export type AdminPairsListRequest = S["AdminPairsListRequest"];
export type AdminPairsListResponse = S["AdminPairsListResponse"];
export type AdminPasswordChangeRequest = S["AdminPasswordChangeRequest"];
export type AdminResourceCreateRequest = S["AdminResourceCreateRequest"];
export type AdminResourceCreateResponse = S["AdminResourceCreateResponse"];
export type AdminResourceDeleteBlockers = S["AdminResourceDeleteBlockers"];
export type AdminResourceDeleteRequest = S["AdminResourceDeleteRequest"];
export type AdminResourceDeleteResponse = S["AdminResourceDeleteResponse"];
export type AdminResourceRenameRequest = S["AdminResourceRenameRequest"];
export type AdminResourceRenameResponse = S["AdminResourceRenameResponse"];
export type AdminResourcesListRequest = S["AdminResourcesListRequest"];
export type AdminResourcesListResponse = S["AdminResourcesListResponse"];
export type AdminSecretDeleteRequest = S["AdminSecretDeleteRequest"];
export type AdminSecretDeleteResponse = S["AdminSecretDeleteResponse"];
export type AdminSecretEntry = S["AdminSecretEntry"];
export type AdminSecretListRequest = S["AdminSecretListRequest"];
export type AdminSecretListResponse = S["AdminSecretListResponse"];
export type AdminSecretReadRequest = S["AdminSecretReadRequest"];
export type AdminSecretReadResponse = S["AdminSecretReadResponse"];
export type AdminSecretRenameRequest = S["AdminSecretRenameRequest"];
export type AdminSecretWriteRequest = S["AdminSecretWriteRequest"];
export type AdminStoreActionState = S["AdminStoreActionState"];
export type AdminStoreBackendKind = S["AdminStoreBackendKind"];
export type AdminStoreCreateRequest = S["AdminStoreCreateRequest"];
export type AdminStoreCreateResponse = S["AdminStoreCreateResponse"];
export type AdminStoreDeleteResponse = S["AdminStoreDeleteResponse"];
export type AdminStoreListItem = S["AdminStoreListItem"];
export type AdminStoreRotateKeyResponse = S["AdminStoreRotateKeyResponse"];
export type AdminStoreUnlockRequest = S["AdminStoreUnlockRequest"];
export type AdminStoreUpdateRequest = S["AdminStoreUpdateRequest"];
export type AdminStoreUsageSummary = S["AdminStoreUsageSummary"];
export type AdminStoresListResponse = S["AdminStoresListResponse"];
export type AdminUserCreateRequest = S["AdminUserCreateRequest"];
export type AdminUserListItem = S["AdminUserListItem"];
export type AdminUserResetPasswordRequest = S["AdminUserResetPasswordRequest"];
export type AdminUserUpdateRequest = S["AdminUserUpdateRequest"];
export type AdminUsersListResponse = S["AdminUsersListResponse"];
export type AdminWrapperPermissionDeleteRequest = S["AdminWrapperPermissionDeleteRequest"];
export type AdminWrapperPermissionKey = S["AdminWrapperPermissionKey"];
export type AdminWrapperPermissionUpsertRequest = S["AdminWrapperPermissionUpsertRequest"];
export type AdminWrapperPermissionsListRequest = S["AdminWrapperPermissionsListRequest"];
export type AdminWrapperPermissionsListResponse = S["AdminWrapperPermissionsListResponse"];
export type BootstrapRequest = S["BootstrapRequest"];
export type BootstrapResponse = S["BootstrapResponse"];
export type BootstrapStatusResponse = S["BootstrapStatusResponse"];
export type EntityCreator = S["EntityCreator"];
export type LoginRequest = S["LoginRequest"];
export type LoginResponse = S["LoginResponse"];
export type ResourceRecord = S["ResourceRecord"];
export type ResourceSlot = S["ResourceSlot"];
export type ServiceWrapperResourcePair = S["ServiceWrapperResourcePair"];
export type WhoamiResponse = S["WhoamiResponse"];
export type WrapperApprovalMode = S["WrapperApprovalMode"];
export type WrapperPermission = S["WrapperPermission"];
export type WrapperResourcePairRecord = S["WrapperResourcePairRecord"];

// ----- Per-endpoint helpers -----

export function fetchWhoami() {
  return unwrap(client.GET("/v1/admin/auth/whoami"));
}

export function fetchBootstrapStatus() {
  return unwrap(client.GET("/v1/admin/auth/bootstrap/status"));
}

export function postLogin(body: LoginRequest) {
  return unwrap(client.POST("/v1/admin/auth/login", { body }));
}

export function postLogout() {
  return unwrap(client.POST("/v1/admin/auth/logout"));
}

export function postBootstrap(body: BootstrapRequest) {
  return unwrap(client.POST("/v1/admin/auth/bootstrap", { body }));
}

export function postPasswordChange(body: AdminPasswordChangeRequest) {
  return unwrap(client.POST("/v1/admin/auth/password", { body }));
}

// ----- Stores admin -----

export function fetchStores() {
  return unwrap(client.GET("/v1/admin/stores"));
}

export function postStoreCreate(body: AdminStoreCreateRequest) {
  return unwrap(client.POST("/v1/admin/stores", { body }));
}

export function postStoreRotateKey(storeId: string) {
  return unwrap(
    client.POST("/v1/admin/stores/{name}/rotate-key", {
      params: { path: { name: storeId } },
    }),
  );
}

export function postStoreUnlock(
  storeId: string,
  body: AdminStoreUnlockRequest,
) {
  return unwrap(
    client.POST("/v1/admin/stores/{name}/unlock", {
      params: { path: { name: storeId } },
      body,
    }),
  );
}

export function postStoreLock(storeId: string) {
  return unwrap(
    client.POST("/v1/admin/stores/{name}/lock", {
      params: { path: { name: storeId } },
    }),
  );
}

export function putStoreUpdate(
  storeId: string,
  body: AdminStoreUpdateRequest,
) {
  return unwrap(
    client.PUT("/v1/admin/stores/{name}", {
      params: { path: { name: storeId } },
      body,
    }),
  );
}

export function deleteStore(storeId: string, opts: { force?: boolean } = {}) {
  return unwrap(
    client.DELETE("/v1/admin/stores/{name}", {
      params: {
        path: { name: storeId },
        query: opts.force ? { force: "true" } : undefined,
      },
    }),
  );
}

// ----- Client instances admin -----

export function fetchClientInstances() {
  return unwrap(client.GET("/v1/admin/client-instances"));
}

export function postClientInstanceCreate(
  body: AdminClientInstanceCreateRequest,
) {
  return unwrap(client.POST("/v1/admin/client-instances", { body }));
}

export function putClientInstanceUpdate(
  id: string,
  body: AdminClientInstanceUpdateRequest,
) {
  return unwrap(
    client.PUT("/v1/admin/client-instances/{id}", {
      params: { path: { id } },
      body,
    }),
  );
}

export function deleteClientInstance(id: string) {
  return unwrap(
    client.DELETE("/v1/admin/client-instances/{id}", {
      params: { path: { id } },
    }),
  );
}

// ----- Users admin -----

export function fetchUsers() {
  return unwrap(client.GET("/v1/admin/users"));
}

export function postUserCreate(body: AdminUserCreateRequest) {
  return unwrap(client.POST("/v1/admin/users", { body }));
}

export function putUserUpdate(id: string, body: AdminUserUpdateRequest) {
  return unwrap(
    client.PUT("/v1/admin/users/{id}", { params: { path: { id } }, body }),
  );
}

export function postUserResetPassword(
  id: string,
  body: AdminUserResetPasswordRequest,
) {
  return unwrap(
    client.POST("/v1/admin/users/{id}/password", {
      params: { path: { id } },
      body,
    }),
  );
}

export function deleteUser(id: string) {
  return unwrap(
    client.DELETE("/v1/admin/users/{id}", { params: { path: { id } } }),
  );
}

// ----- Wrapper permissions / policy -----

export function postWrapperPermissionsList(
  body: AdminWrapperPermissionsListRequest,
) {
  return unwrap(
    client.POST("/v1/admin/wrapper-permissions/list", { body }),
  );
}

export function postWrapperPermissionUpsert(
  body: AdminWrapperPermissionUpsertRequest,
) {
  return unwrap(
    client.POST("/v1/admin/wrapper-permissions/upsert", { body }),
  );
}

export function postWrapperPermissionDelete(
  body: AdminWrapperPermissionDeleteRequest,
) {
  return unwrap(
    client.POST("/v1/admin/wrapper-permissions/delete", { body }),
  );
}

// ----- Secrets CRUD -----

export function postSecretList(body: AdminSecretListRequest) {
  return unwrap(client.POST("/v1/admin/secret/list", { body }));
}

export function postSecretRead(body: AdminSecretReadRequest) {
  return unwrap(client.POST("/v1/admin/secret/read", { body }));
}

export function postSecretWrite(body: AdminSecretWriteRequest) {
  return unwrap(client.POST("/v1/admin/secret/write", { body }));
}

export function postSecretDelete(body: AdminSecretDeleteRequest) {
  return unwrap(client.POST("/v1/admin/secret/delete", { body }));
}

export function postSecretRename(body: AdminSecretRenameRequest) {
  return unwrap(client.POST("/v1/admin/secret/rename", { body }));
}

// ----- Resources and wrapper/resource pairs -----

export function postResourcesList(body: AdminResourcesListRequest) {
  return unwrap(client.POST("/v1/admin/resources/list", { body }));
}

export function postResourceCreate(body: AdminResourceCreateRequest) {
  return unwrap(client.POST("/v1/admin/resources/create", { body }));
}

export function postResourceDelete(body: AdminResourceDeleteRequest) {
  return unwrap(client.POST("/v1/admin/resources/delete", { body }));
}

export function postResourceRename(body: AdminResourceRenameRequest) {
  return unwrap(client.POST("/v1/admin/resources/rename", { body }));
}

export function postPairsList(body: AdminPairsListRequest) {
  return unwrap(client.POST("/v1/admin/pairs/list", { body }));
}

export function postPairCreate(body: AdminPairCreateRequest) {
  return unwrap(client.POST("/v1/admin/pairs/create", { body }));
}

export function postPairDelete(body: AdminPairDeleteRequest) {
  return unwrap(client.POST("/v1/admin/pairs/delete", { body }));
}
