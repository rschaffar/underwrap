import { api, type S } from "./core";

export const fetchStores = () => api.GET("/v1/admin/stores");

export const postStoreKeyGenerate = () =>
  api.POST("/v1/admin/store-keys");

export const postStoreCreate = (body: S["AdminStoreCreateRequest"]) =>
  api.POST("/v1/admin/stores", { body });

export const postStoreMigrate = (
  storeId: string,
  body: S["AdminStoreMigrateRequest"],
) =>
  api.POST("/v1/admin/stores/{id}/migrate", {
    params: { path: { id: storeId } },
    body,
  });

export const postStoreUnlock = (
  storeId: string,
  body: S["AdminStoreUnlockRequest"],
) =>
  api.POST("/v1/admin/stores/{id}/unlock", {
    params: { path: { id: storeId } },
    body,
  });

export const putStoreUpdate = (
  storeId: string,
  body: S["AdminStoreUpdateRequest"],
) =>
  api.PUT("/v1/admin/stores/{id}", {
    params: { path: { id: storeId } },
    body,
  });

export const deleteStore = (storeId: string, opts: { force?: boolean } = {}) =>
  api.DELETE("/v1/admin/stores/{id}", {
    params: { path: { id: storeId }, query: { force: opts.force ?? false } },
  });

export const postStoreRotateKey = (storeId: string) =>
  api.POST("/v1/admin/stores/{id}/rotate-key", {
    params: { path: { id: storeId } },
  });

export const postStoreLock = (storeId: string) =>
  api.POST("/v1/admin/stores/{id}/lock", {
    params: { path: { id: storeId } },
  });
