import { api, type S } from "./core";

type KvSecretSlot = S["KvSecretSlot"];

export const fetchSecret = (
  storeId: string,
  rid: string,
  slot: KvSecretSlot,
  key: string,
) =>
  api.GET("/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}", {
    params: { path: { id: storeId, rid, slot, key } },
  });

export const fetchSecretsView = (storeId: string) =>
  api.GET("/v1/admin/stores/{id}/secrets-view", {
    params: { path: { id: storeId } },
  });

export const putSecret = (
  storeId: string,
  rid: string,
  slot: KvSecretSlot,
  key: string,
  body: S["AdminSecretWriteRequest"],
) =>
  api.PUT("/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}", {
    params: { path: { id: storeId, rid, slot, key } },
    body,
  });

export const postSecretRename = (
  storeId: string,
  rid: string,
  slot: KvSecretSlot,
  fromKey: string,
  body: S["AdminSecretRenameRequest"],
) =>
  api.POST("/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}/rename", {
    params: { path: { id: storeId, rid, slot, key: fromKey } },
    body,
  });

export const deleteSecret = (
  storeId: string,
  rid: string,
  slot: KvSecretSlot,
  key: string,
) =>
  api.DELETE("/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}", {
    params: { path: { id: storeId, rid, slot, key } },
  });
