/**
 * Public entry point for the typed admin-API client.
 *
 * The cross-cutting plumbing (openapi-fetch setup, CSRF capture/replay,
 * `ApiError`, the `api.GET/POST/PUT/DELETE` surface, and the named
 * schema-type re-exports) lives in `./core`. The endpoint wrappers are
 * grouped by domain in sibling modules so adding an endpoint touches
 * only its domain file, not this barrel. Call sites keep importing
 * everything from `@/api/client`.
 */
export * from "./core";
export * from "./auth";
export * from "./stores";
export * from "./client-instances";
export * from "./secrets";
export * from "./resources";
export * from "./pairs";
export * from "./wrapper-permissions";
export * from "./libsecret";
export * from "./users";
