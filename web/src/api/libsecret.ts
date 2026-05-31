import { api, type S } from "./core";

export const fetchLibsecretTree = (storeId: string, rid: string) =>
  api.GET("/v1/admin/stores/{sid}/resources/{rid}/libsecret", {
    params: { path: { sid: storeId, rid } },
  });

export const fetchLibsecretItemSecret = (
  storeId: string,
  rid: string,
  cid: string,
  iid: string,
) =>
  api.GET(
    "/v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items/{iid}/secret",
    { params: { path: { sid: storeId, rid, cid, iid } } },
  );

export const postLibsecretItem = (
  storeId: string,
  rid: string,
  cid: string,
  body: S["AdminLibsecretItemCreateRequest"],
) =>
  api.POST(
    "/v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items",
    {
      params: { path: { sid: storeId, rid, cid } },
      body,
    },
  );

export const postLibsecretCollection = (
  storeId: string,
  rid: string,
  body: S["AdminLibsecretCollectionCreateRequest"],
) =>
  api.POST("/v1/admin/stores/{sid}/resources/{rid}/libsecret/collections", {
    params: { path: { sid: storeId, rid } },
    body,
  });

export const deleteLibsecretCollection = (
  storeId: string,
  rid: string,
  cid: string,
) =>
  api.DELETE(
    "/v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}",
    {
      params: { path: { sid: storeId, rid, cid } },
    },
  );

export const deleteLibsecretItem = (
  storeId: string,
  rid: string,
  cid: string,
  iid: string,
) =>
  api.DELETE(
    "/v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items/{iid}",
    { params: { path: { sid: storeId, rid, cid, iid } } },
  );

export const putLibsecretAlias = (
  storeId: string,
  rid: string,
  name: string,
  body: S["AdminLibsecretAliasUpsertRequest"],
) =>
  api.PUT("/v1/admin/stores/{sid}/resources/{rid}/libsecret/aliases/{name}", {
    params: { path: { sid: storeId, rid, name } },
    body,
  });

export const deleteLibsecretAlias = (
  storeId: string,
  rid: string,
  name: string,
) =>
  api.DELETE(
    "/v1/admin/stores/{sid}/resources/{rid}/libsecret/aliases/{name}",
    {
      params: { path: { sid: storeId, rid, name } },
    },
  );
