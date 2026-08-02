import { api, type S } from "./core";

export const postPairCreate = (
  storeId: string,
  body: S["AdminPairCreateRequest"],
) =>
  api.POST("/v1/admin/stores/{id}/pairs", {
    params: { path: { id: storeId } },
    body,
  });

export const postPairWithNewResource = (
  storeId: string,
  body: S["AdminPairWithNewResourceRequest"],
) =>
  api.POST("/v1/admin/stores/{id}/pairs/with-new-resource", {
    params: { path: { id: storeId } },
    body,
  });

export const deletePair = (
  storeId: string,
  wrapper: string,
  resourceId: string,
) =>
  api.DELETE("/v1/admin/stores/{id}/pairs/{wrapper}/{rid}", {
    params: { path: { id: storeId, wrapper, rid: resourceId } },
  });
