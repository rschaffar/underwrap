/**
 * Typed admin-API client.
 *
 * Built on top of `openapi-fetch` against the schema generated from
 * `crates/backend-backendd/openapi.json` (see `npm run generate:api`).
 * The schema is the source of truth — every request body, response
 * shape, and path is inferred from it.
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
 *
 * The public surface is `api.GET / POST / PUT / DELETE` — thin
 * wrappers that compose `unwrap(client.X(...))` once. Call sites pass
 * the typed schema path and rely on openapi-fetch's path/body
 * inference for safety. The single wrap point keeps auth and CSRF
 * behaviour centralised.
 */

import createClient, {
  type FetchResponse,
  type MaybeOptionalInit,
  type Middleware,
} from "openapi-fetch";
import type {
  MediaType,
  PathsWithMethod,
  RequiredKeysOf,
} from "openapi-typescript-helpers";

// `InitParam` is not exported from openapi-fetch; inline the same
// shape so call sites can omit the init object when every field is
// optional, but are forced to pass one when a body or params are
// required. Mirrors the type-spelling in `node_modules/openapi-fetch`.
type InitParam<Init> =
  RequiredKeysOf<Init> extends never
    ? [(Init & { [key: string]: unknown })?]
    : [Init & { [key: string]: unknown }];

import type { components, paths } from "./schema";

// Convenience: the `data` payload an openapi-fetch call resolves to,
// or `undefined` for 204-style endpoints. Used to spell the return
// type of every `api.X` method.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyPathInfo = Record<string | number, any>;

// Data payload an openapi-fetch call resolves to on a 2xx response.
// `api.X` throws on non-2xx, so we narrow with `NonNullable` —
// callers can rely on the body the schema declares for the 2xx
// branch. 204 endpoints have `data: never` in the schema and
// `NonNullable<never>` stays `never`; call sites for those discard
// the return value anyway.
type UnwrappedData<
  P extends keyof paths,
  M extends keyof paths[P],
  Init,
> = paths[P][M] extends AnyPathInfo
  ? NonNullable<FetchResponse<paths[P][M], Init, MediaType>["data"]>
  : never;

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

// Test-only hook: lets the vitest setup clear the module-scoped CSRF
// token between cases. Gated on Vite's test mode so production
// bundles never see the export.
export const _resetCsrfTokenForTesting: () => void =
  import.meta.env.MODE === "test"
    ? () => {
        csrfToken = null;
      }
    : () => {};

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
// URL is set in `vite.config.ts`).
const apiBaseUrl = window.location.origin;

const client = createClient<paths>({
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

/**
 * Resolve an openapi-fetch call to its raw `data`, or throw an
 * [`ApiError`] carrying the structured `{code, message}` envelope on
 * non-2xx responses. For 204-style endpoints `data` is `undefined`;
 * call sites for those endpoints discard the return value.
 */
async function unwrap<T>(
  call: Promise<{ data?: T; error?: unknown; response: Response }>,
): Promise<T> {
  const { data, error, response } = await call;
  if (response.ok) {
    return data as T;
  }
  const envelope = (error ?? {}) as { code?: string; message?: string };
  throw new ApiError(
    response.status,
    envelope.code ?? "error",
    envelope.message ?? response.statusText,
  );
}

/**
 * Convenience surface used by all call sites: `api.GET("/v1/admin/...")`
 * etc. The generics flow through from openapi-fetch's typed methods,
 * so the path string, params object, and request body remain fully
 * type-checked against the schema.
 */
export const api = {
  GET<
    P extends PathsWithMethod<paths, "get">,
    Init extends MaybeOptionalInit<paths[P], "get">,
  >(url: P, ...init: InitParam<Init>): Promise<UnwrappedData<P, "get", Init>> {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    return unwrap(client.GET(url, ...(init as any))) as Promise<
      UnwrappedData<P, "get", Init>
    >;
  },
  POST<
    P extends PathsWithMethod<paths, "post">,
    Init extends MaybeOptionalInit<paths[P], "post">,
  >(url: P, ...init: InitParam<Init>): Promise<UnwrappedData<P, "post", Init>> {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    return unwrap(client.POST(url, ...(init as any))) as Promise<
      UnwrappedData<P, "post", Init>
    >;
  },
  PUT<
    P extends PathsWithMethod<paths, "put">,
    Init extends MaybeOptionalInit<paths[P], "put">,
  >(url: P, ...init: InitParam<Init>): Promise<UnwrappedData<P, "put", Init>> {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    return unwrap(client.PUT(url, ...(init as any))) as Promise<
      UnwrappedData<P, "put", Init>
    >;
  },
  DELETE<
    P extends PathsWithMethod<paths, "delete">,
    Init extends MaybeOptionalInit<paths[P], "delete">,
  >(
    url: P,
    ...init: InitParam<Init>
  ): Promise<UnwrappedData<P, "delete", Init>> {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    return unwrap(client.DELETE(url, ...(init as any))) as Promise<
      UnwrappedData<P, "delete", Init>
    >;
  },
};

// ----- Named schema types -----
//
// Re-exports for the schemas that page / dialog code references by
// name. Anything else can be reached through `components["schemas"]`.
export type S = components["schemas"];
export type AdminClientInstanceListItem = S["AdminClientInstanceListItem"];
export type AdminClientInstancesListResponse =
  S["AdminClientInstancesListResponse"];
export type AdminLibsecretCollectionView = S["AdminLibsecretCollectionView"];
export type AdminLibsecretItemView = S["AdminLibsecretItemView"];
export type AdminLibsecretTreeResponse = S["AdminLibsecretTreeResponse"];
export type AdminPairsListResponse = S["AdminPairsListResponse"];
export type AdminResourcesListResponse = S["AdminResourcesListResponse"];
export type AdminSecretEntry = S["AdminSecretEntry"];
export type AdminSecretListResponse = S["AdminSecretListResponse"];
export type AdminSecretReadResponse = S["AdminSecretReadResponse"];
export type AdminSecretsView = S["AdminSecretsView"];
export type AdminStoreCreateRequest = S["AdminStoreCreateRequest"];
export type AdminStoreCreateResponse = S["AdminStoreCreateResponse"];
export type AdminStoreListItem = S["AdminStoreListItem"];
export type AdminStoresListResponse = S["AdminStoresListResponse"];
export type AdminUserCreateRequest = S["AdminUserCreateRequest"];
export type AdminUserListItem = S["AdminUserListItem"];
export type AdminUsersListResponse = S["AdminUsersListResponse"];
export type AdminWrapperPermissionsListResponse =
  S["AdminWrapperPermissionsListResponse"];
export type AdminWrapperPermissionsView = S["AdminWrapperPermissionsView"];
export type AdminClientInstanceCreateRequest =
  S["AdminClientInstanceCreateRequest"];
export type EntityCreator = S["EntityCreator"];
export type KvSecretSlot = S["KvSecretSlot"];
export type ResourceRecord = S["ResourceRecord"];
export type WhoamiResponse = S["WhoamiResponse"];
export type WrapperApprovalMode = S["WrapperApprovalMode"];
export type WrapperPermission = S["WrapperPermission"];
export type WrapperResourcePairRecord = S["WrapperResourcePairRecord"];
