import { api, type S } from "./core";

export const fetchUsers = () => api.GET("/v1/admin/users");

export const postUserCreate = (body: S["AdminUserCreateRequest"]) =>
  api.POST("/v1/admin/users", { body });

export const putUserUpdate = (id: string, body: S["AdminUserUpdateRequest"]) =>
  api.PUT("/v1/admin/users/{id}", { params: { path: { id } }, body });

export const postUserResetPassword = (
  id: string,
  body: S["AdminUserResetPasswordRequest"],
) =>
  api.POST("/v1/admin/users/{id}/password", {
    params: { path: { id } },
    body,
  });

export const deleteUser = (id: string) =>
  api.DELETE("/v1/admin/users/{id}", { params: { path: { id } } });
