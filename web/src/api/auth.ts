import { api, type S } from "./core";

export const fetchWhoami = () => api.GET("/v1/admin/auth/whoami");
export const fetchBootstrapStatus = () =>
  api.GET("/v1/admin/auth/bootstrap/status");
export const postLogin = (body: S["LoginRequest"]) =>
  api.POST("/v1/admin/auth/login", { body });
export const postLogout = () => api.POST("/v1/admin/auth/logout");
export const postBootstrap = (body: S["BootstrapRequest"]) =>
  api.POST("/v1/admin/auth/bootstrap", { body });
export const postPasswordChange = (body: S["AdminPasswordChangeRequest"]) =>
  api.POST("/v1/admin/auth/password", { body });
