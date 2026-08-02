import { api, type S } from "./core";

export const postResourceCreate = (
  storeId: string,
  body: S["AdminResourceCreateRequest"],
) =>
  api.POST("/v1/admin/stores/{id}/resources", {
    params: { path: { id: storeId } },
    body,
  });

export const putResourceRename = (
  storeId: string,
  resourceId: string,
  body: S["AdminResourceRenameRequest"],
) =>
  api.PUT("/v1/admin/stores/{id}/resources/{rid}", {
    params: { path: { id: storeId, rid: resourceId } },
    body,
  });

export const deleteResource = (storeId: string, resourceId: string) =>
  api.DELETE("/v1/admin/stores/{id}/resources/{rid}", {
    params: { path: { id: storeId, rid: resourceId } },
  });
