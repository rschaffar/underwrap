import { api, type S } from "./core";

export const fetchClientInstances = () => api.GET("/v1/admin/client-instances");

export const postClientInstanceCreate = (
  body: S["AdminClientInstanceCreateRequest"],
) => api.POST("/v1/admin/client-instances", { body });

export const putClientInstanceUpdate = (
  id: string,
  body: S["AdminClientInstanceUpdateRequest"],
) =>
  api.PUT("/v1/admin/client-instances/{id}", {
    params: { path: { id } },
    body,
  });

export const deleteClientInstance = (id: string) =>
  api.DELETE("/v1/admin/client-instances/{id}", {
    params: { path: { id } },
  });
