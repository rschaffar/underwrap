import { api, type S } from "./core";

export const fetchWrapperPermissionsView = (storeId: string) =>
  api.GET("/v1/admin/stores/{id}/wrapper-permissions-view", {
    params: { path: { id: storeId } },
  });

export const putWrapperPermission = (
  storeId: string,
  clientInstance: string,
  wrapper: string,
  resourceId: string,
  body: S["AdminWrapperPermissionUpsertRequest"],
) =>
  api.PUT(
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
  );

export const deleteWrapperPermission = (
  storeId: string,
  clientInstance: string,
  wrapper: string,
  resourceId: string,
) =>
  api.DELETE(
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
  );
