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

// ----- Named schema types -----
//
// Aliases for the openapi-typescript `components["schemas"]` entries.
// The `export` ones are read from outside `web/src/api/`; the
// non-exported ones are referenced inside this file as parameter
// annotations on the per-endpoint helpers. Anything that's never
// referenced has been deleted — re-add only when a real callsite
// surfaces.
type S = components["schemas"];
export type AdminClientInstanceCreateRequest = S["AdminClientInstanceCreateRequest"];
export type AdminClientInstanceListItem = S["AdminClientInstanceListItem"];
type AdminClientInstanceUpdateRequest = S["AdminClientInstanceUpdateRequest"];
export type AdminClientInstancesListResponse = S["AdminClientInstancesListResponse"];
type AdminPairCreateRequest = S["AdminPairCreateRequest"];
export type AdminPairsListResponse = S["AdminPairsListResponse"];
type AdminPasswordChangeRequest = S["AdminPasswordChangeRequest"];
type AdminResourceCreateRequest = S["AdminResourceCreateRequest"];
type AdminResourceRenameRequest = S["AdminResourceRenameRequest"];
export type AdminResourcesListResponse = S["AdminResourcesListResponse"];
export type AdminSecretEntry = S["AdminSecretEntry"];
export type AdminSecretListResponse = S["AdminSecretListResponse"];
export type AdminSecretReadResponse = S["AdminSecretReadResponse"];
type AdminSecretRenameRequest = S["AdminSecretRenameRequest"];
type AdminSecretWriteRequest = S["AdminSecretWriteRequest"];
export type AdminSecretsView = S["AdminSecretsView"];
export type AdminStoreCreateRequest = S["AdminStoreCreateRequest"];
export type AdminStoreListItem = S["AdminStoreListItem"];
type AdminStoreUnlockRequest = S["AdminStoreUnlockRequest"];
type AdminStoreUpdateRequest = S["AdminStoreUpdateRequest"];
export type AdminStoresListResponse = S["AdminStoresListResponse"];
export type AdminUserCreateRequest = S["AdminUserCreateRequest"];
export type AdminUserListItem = S["AdminUserListItem"];
type AdminUserResetPasswordRequest = S["AdminUserResetPasswordRequest"];
type AdminUserUpdateRequest = S["AdminUserUpdateRequest"];
export type AdminUsersListResponse = S["AdminUsersListResponse"];
type AdminWrapperPermissionUpsertRequest = S["AdminWrapperPermissionUpsertRequest"];
export type AdminWrapperPermissionsListResponse = S["AdminWrapperPermissionsListResponse"];
export type AdminWrapperPermissionsView = S["AdminWrapperPermissionsView"];
type BootstrapRequest = S["BootstrapRequest"];
export type EntityCreator = S["EntityCreator"];
type LoginRequest = S["LoginRequest"];
export type ResourceRecord = S["ResourceRecord"];
export type ResourceSlot = S["ResourceSlot"];
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
    client.POST("/v1/admin/stores/{id}/rotate-key", {
      params: { path: { id: storeId } },
    }),
  );
}

export function postStoreUnlock(
  storeId: string,
  body: AdminStoreUnlockRequest,
) {
  return unwrap(
    client.POST("/v1/admin/stores/{id}/unlock", {
      params: { path: { id: storeId } },
      body,
    }),
  );
}

export function postStoreLock(storeId: string) {
  return unwrap(
    client.POST("/v1/admin/stores/{id}/lock", {
      params: { path: { id: storeId } },
    }),
  );
}

export function putStoreUpdate(
  storeId: string,
  body: AdminStoreUpdateRequest,
) {
  return unwrap(
    client.PUT("/v1/admin/stores/{id}", {
      params: { path: { id: storeId } },
      body,
    }),
  );
}

export function deleteStore(storeId: string, opts: { force?: boolean } = {}) {
  return unwrap(
    client.DELETE("/v1/admin/stores/{id}", {
      params: {
        path: { id: storeId },
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

// ----- Per-store: resources, pairs, wrapper-permissions, secrets -----
//
// All four families share the `/v1/admin/stores/{id}/<thing>` shape.
// Identity-bearing fields (resource id, wrapper, secret slot+key) live
// in URL path segments; bodies carry only value-bearing fields.

export function fetchResources(storeId: string) {
  return unwrap(
    client.GET("/v1/admin/stores/{id}/resources", {
      params: { path: { id: storeId } },
    }),
  );
}

export function postResourceCreate(
  storeId: string,
  body: AdminResourceCreateRequest,
) {
  return unwrap(
    client.POST("/v1/admin/stores/{id}/resources", {
      params: { path: { id: storeId } },
      body,
    }),
  );
}

export function deleteResource(storeId: string, resourceId: string) {
  return unwrap(
    client.DELETE("/v1/admin/stores/{id}/resources/{rid}", {
      params: { path: { id: storeId, rid: resourceId } },
    }),
  );
}

export function putResourceRename(
  storeId: string,
  resourceId: string,
  body: AdminResourceRenameRequest,
) {
  return unwrap(
    client.PUT("/v1/admin/stores/{id}/resources/{rid}", {
      params: { path: { id: storeId, rid: resourceId } },
      body,
    }),
  );
}

export function fetchPairs(storeId: string) {
  return unwrap(
    client.GET("/v1/admin/stores/{id}/pairs", {
      params: { path: { id: storeId } },
    }),
  );
}

export function postPairCreate(storeId: string, body: AdminPairCreateRequest) {
  return unwrap(
    client.POST("/v1/admin/stores/{id}/pairs", {
      params: { path: { id: storeId } },
      body,
    }),
  );
}

export function deletePair(
  storeId: string,
  wrapper: string,
  resourceId: string,
) {
  return unwrap(
    client.DELETE("/v1/admin/stores/{id}/pairs/{wrapper}/{rid}", {
      params: { path: { id: storeId, wrapper, rid: resourceId } },
    }),
  );
}

export function fetchWrapperPermissions(storeId: string) {
  return unwrap(
    client.GET("/v1/admin/stores/{id}/wrapper-permissions", {
      params: { path: { id: storeId } },
    }),
  );
}

export function putWrapperPermission(
  storeId: string,
  clientInstance: string,
  wrapper: string,
  resourceId: string,
  body: AdminWrapperPermissionUpsertRequest,
) {
  return unwrap(
    client.PUT(
      "/v1/admin/stores/{id}/wrapper-permissions/{client_instance}/{wrapper}/{rid}",
      {
        params: {
          path: {
            id: storeId,
            client_instance: clientInstance,
            wrapper,
            rid: resourceId,
          },
        },
        body,
      },
    ),
  );
}

export function deleteWrapperPermission(
  storeId: string,
  clientInstance: string,
  wrapper: string,
  resourceId: string,
) {
  return unwrap(
    client.DELETE(
      "/v1/admin/stores/{id}/wrapper-permissions/{client_instance}/{wrapper}/{rid}",
      {
        params: {
          path: {
            id: storeId,
            client_instance: clientInstance,
            wrapper,
            rid: resourceId,
          },
        },
      },
    ),
  );
}

export function fetchSecrets(storeId: string, resourceId?: string) {
  return unwrap(
    client.GET("/v1/admin/stores/{id}/secrets", {
      params: {
        path: { id: storeId },
        query: resourceId ? { resource_id: resourceId } : undefined,
      },
    }),
  );
}

export function fetchSecret(
  storeId: string,
  resourceId: string,
  slot: ResourceSlot,
  key: string,
) {
  return unwrap(
    client.GET("/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}", {
      params: { path: { id: storeId, rid: resourceId, slot, key } },
    }),
  );
}

export function putSecret(
  storeId: string,
  resourceId: string,
  slot: ResourceSlot,
  key: string,
  body: AdminSecretWriteRequest,
) {
  return unwrap(
    client.PUT("/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}", {
      params: { path: { id: storeId, rid: resourceId, slot, key } },
      body,
    }),
  );
}

export function deleteSecret(
  storeId: string,
  resourceId: string,
  slot: ResourceSlot,
  key: string,
) {
  return unwrap(
    client.DELETE("/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}", {
      params: { path: { id: storeId, rid: resourceId, slot, key } },
    }),
  );
}

export function postSecretRename(
  storeId: string,
  resourceId: string,
  slot: ResourceSlot,
  fromKey: string,
  body: AdminSecretRenameRequest,
) {
  return unwrap(
    client.POST(
      "/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}/rename",
      {
        params: { path: { id: storeId, rid: resourceId, slot, key: fromKey } },
        body,
      },
    ),
  );
}

// ----- Aggregate views -----

export function fetchWrapperPermissionsView(storeId: string) {
  return unwrap(
    client.GET("/v1/admin/stores/{id}/wrapper-permissions-view", {
      params: { path: { id: storeId } },
    }),
  );
}

export function fetchSecretsView(storeId: string) {
  return unwrap(
    client.GET("/v1/admin/stores/{id}/secrets-view", {
      params: { path: { id: storeId } },
    }),
  );
}
