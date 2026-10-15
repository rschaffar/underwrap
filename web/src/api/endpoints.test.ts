import { afterEach, describe, expect, it, vi } from "vitest";

import {
  _resetCsrfTokenForTesting,
  deleteClientInstance,
  deleteLibsecretAlias,
  deleteLibsecretCollection,
  deleteLibsecretItem,
  deletePair,
  deleteResource,
  deleteUser,
  deleteWrapperPermission,
  fetchBootstrapStatus,
  fetchClientInstances,
  fetchLibsecretTree,
  fetchSecretsView,
  fetchWrapperPermissionsView,
  postClientInstanceCreate,
  postLibsecretCollection,
  postLibsecretItem,
  postPairCreate,
  postPairWithNewResource,
  postPasswordChange,
  postResourceCreate,
  postSecretRename,
  postStoreCreate,
  postStoreKeyGenerate,
  postStoreMigrate,
  postUserCreate,
  postUserResetPassword,
  putClientInstanceUpdate,
  putLibsecretAlias,
  putResourceRename,
  putSecret,
  putStoreUpdate,
  putUserUpdate,
  putWrapperPermission,
} from "./client";

type EndpointCase = {
  name: string;
  invoke: () => Promise<unknown>;
  method: "GET" | "POST" | "PUT" | "DELETE";
  pathname: string;
  body?: unknown;
};

const endpointCases: EndpointCase[] = [
  {
    name: "fetches bootstrap status",
    invoke: fetchBootstrapStatus,
    method: "GET",
    pathname: "/v1/admin/auth/bootstrap/status",
  },
  {
    name: "changes the current operator password",
    invoke: () =>
      postPasswordChange({
        current_password: "synthetic-current-password",
        new_password: "synthetic-replacement-password",
      }),
    method: "POST",
    pathname: "/v1/admin/auth/password",
    body: {
      current_password: "synthetic-current-password",
      new_password: "synthetic-replacement-password",
    },
  },
  {
    name: "lists runtime client instances",
    invoke: fetchClientInstances,
    method: "GET",
    pathname: "/v1/admin/client-instances",
  },
  {
    name: "creates a runtime client instance",
    invoke: () =>
      postClientInstanceCreate({
        auto_create_wrapper_assets: true,
        cert_pem: "synthetic-certificate",
        current_store_id: "store-a",
        display_name: "Client A",
        status: "active",
      }),
    method: "POST",
    pathname: "/v1/admin/client-instances",
    body: {
      auto_create_wrapper_assets: true,
      cert_pem: "synthetic-certificate",
      current_store_id: "store-a",
      display_name: "Client A",
      status: "active",
    },
  },
  {
    name: "updates a runtime client instance",
    invoke: () =>
      putClientInstanceUpdate("client-a", {
        display_name: "Client B",
        status: "disabled",
      }),
    method: "PUT",
    pathname: "/v1/admin/client-instances/client-a",
    body: { display_name: "Client B", status: "disabled" },
  },
  {
    name: "deletes a runtime client instance",
    invoke: () => deleteClientInstance("client-a"),
    method: "DELETE",
    pathname: "/v1/admin/client-instances/client-a",
  },
  {
    name: "fetches the libsecret tree",
    invoke: () => fetchLibsecretTree("store-a", "resource-a"),
    method: "GET",
    pathname: "/v1/admin/stores/store-a/resources/resource-a/libsecret",
  },
  {
    name: "creates a libsecret item",
    invoke: () =>
      postLibsecretItem("store-a", "resource-a", "collection_a", {
        attributes: { scope: "test" },
        content_type: "text/plain",
        id: "0123456789abcdef0123456789abcdef",
        label: "Item",
        secret_b64: "c3ludGhldGlj",
      }),
    method: "POST",
    pathname:
      "/v1/admin/stores/store-a/resources/resource-a/libsecret/collections/collection_a/items",
    body: {
      attributes: { scope: "test" },
      content_type: "text/plain",
      id: "0123456789abcdef0123456789abcdef",
      label: "Item",
      secret_b64: "c3ludGhldGlj",
    },
  },
  {
    name: "creates a libsecret collection",
    invoke: () =>
      postLibsecretCollection("store-a", "resource-a", {
        id: "collection_new",
        label: "Collection",
      }),
    method: "POST",
    pathname:
      "/v1/admin/stores/store-a/resources/resource-a/libsecret/collections",
    body: { id: "collection_new", label: "Collection" },
  },
  {
    name: "deletes a libsecret collection",
    invoke: () =>
      deleteLibsecretCollection("store-a", "resource-a", "collection-a"),
    method: "DELETE",
    pathname:
      "/v1/admin/stores/store-a/resources/resource-a/libsecret/collections/collection-a",
  },
  {
    name: "deletes a libsecret item",
    invoke: () =>
      deleteLibsecretItem(
        "store-a",
        "resource-a",
        "collection-a",
        "item-a",
      ),
    method: "DELETE",
    pathname:
      "/v1/admin/stores/store-a/resources/resource-a/libsecret/collections/collection-a/items/item-a",
  },
  {
    name: "updates a libsecret alias",
    invoke: () =>
      putLibsecretAlias("store-a", "resource-a", "default", {
        collection_id: "collection_a",
      }),
    method: "PUT",
    pathname:
      "/v1/admin/stores/store-a/resources/resource-a/libsecret/aliases/default",
    body: { collection_id: "collection_a" },
  },
  {
    name: "deletes a libsecret alias",
    invoke: () =>
      deleteLibsecretAlias("store-a", "resource-a", "default"),
    method: "DELETE",
    pathname:
      "/v1/admin/stores/store-a/resources/resource-a/libsecret/aliases/default",
  },
  {
    name: "creates a wrapper/resource pair",
    invoke: () =>
      postPairCreate("store-a", {
        resource_id: "resource-a",
        wrapper: "wrapper-a",
      }),
    method: "POST",
    pathname: "/v1/admin/stores/store-a/pairs",
    body: { resource_id: "resource-a", wrapper: "wrapper-a" },
  },
  {
    name: "creates a pair with a new resource",
    invoke: () =>
      postPairWithNewResource("store-a", {
        resource_name: "resource/new",
        wrapper: "wrapper-a",
      }),
    method: "POST",
    pathname: "/v1/admin/stores/store-a/pairs/with-new-resource",
    body: { resource_name: "resource/new", wrapper: "wrapper-a" },
  },
  {
    name: "deletes a wrapper/resource pair",
    invoke: () => deletePair("store-a", "wrapper-a", "resource-a"),
    method: "DELETE",
    pathname: "/v1/admin/stores/store-a/pairs/wrapper-a/resource-a",
  },
  {
    name: "creates a resource",
    invoke: () => postResourceCreate("store-a", { name: "resource/new" }),
    method: "POST",
    pathname: "/v1/admin/stores/store-a/resources",
    body: { name: "resource/new" },
  },
  {
    name: "renames a resource",
    invoke: () =>
      putResourceRename("store-a", "resource-a", {
        new_name: "resource/renamed",
      }),
    method: "PUT",
    pathname: "/v1/admin/stores/store-a/resources/resource-a",
    body: { new_name: "resource/renamed" },
  },
  {
    name: "deletes a resource",
    invoke: () => deleteResource("store-a", "resource-a"),
    method: "DELETE",
    pathname: "/v1/admin/stores/store-a/resources/resource-a",
  },
  {
    name: "fetches the secrets view",
    invoke: () => fetchSecretsView("store-a"),
    method: "GET",
    pathname: "/v1/admin/stores/store-a/secrets-view",
  },
  {
    name: "writes a secret entry",
    invoke: () =>
      putSecret("store-a", "resource-a", "files", "token", {
        material_b64: "c3ludGhldGlj",
      }),
    method: "PUT",
    pathname: "/v1/admin/stores/store-a/secrets/resource-a/files/token",
    body: { material_b64: "c3ludGhldGlj" },
  },
  {
    name: "renames a secret entry",
    invoke: () =>
      postSecretRename("store-a", "resource-a", "env", "OLD_NAME", {
        material_b64: null,
        to_key: "NEW_NAME",
      }),
    method: "POST",
    pathname:
      "/v1/admin/stores/store-a/secrets/resource-a/env/OLD_NAME/rename",
    body: { material_b64: null, to_key: "NEW_NAME" },
  },
  {
    name: "generates a storage-neutral store key",
    invoke: postStoreKeyGenerate,
    method: "POST",
    pathname: "/v1/admin/store-keys",
  },
  {
    name: "creates a plain store without a key",
    invoke: () => postStoreCreate({ backend: "plain", name: "Store A" }),
    method: "POST",
    pathname: "/v1/admin/stores",
    body: { backend: "plain", name: "Store A" },
  },
  {
    name: "migrates a store to plain without a key",
    invoke: () => postStoreMigrate("store-a", { backend: "plain" }),
    method: "POST",
    pathname: "/v1/admin/stores/store-a/migrate",
    body: { backend: "plain" },
  },
  {
    name: "updates a store",
    invoke: () =>
      putStoreUpdate("store-a", {
        description: "Updated description",
        name: "Store B",
      }),
    method: "PUT",
    pathname: "/v1/admin/stores/store-a",
    body: { description: "Updated description", name: "Store B" },
  },
  {
    name: "creates an operator",
    invoke: () =>
      postUserCreate({
        password: "synthetic-operator-password",
        username: "operator-a",
      }),
    method: "POST",
    pathname: "/v1/admin/users",
    body: {
      password: "synthetic-operator-password",
      username: "operator-a",
    },
  },
  {
    name: "updates an operator",
    invoke: () => putUserUpdate("user-a", { username: "operator-b" }),
    method: "PUT",
    pathname: "/v1/admin/users/user-a",
    body: { username: "operator-b" },
  },
  {
    name: "resets an operator password",
    invoke: () =>
      postUserResetPassword("user-a", {
        password: "synthetic-reset-password",
      }),
    method: "POST",
    pathname: "/v1/admin/users/user-a/password",
    body: { password: "synthetic-reset-password" },
  },
  {
    name: "deletes an operator",
    invoke: () => deleteUser("user-a"),
    method: "DELETE",
    pathname: "/v1/admin/users/user-a",
  },
  {
    name: "fetches the wrapper-permissions view",
    invoke: () => fetchWrapperPermissionsView("store-a"),
    method: "GET",
    pathname: "/v1/admin/stores/store-a/wrapper-permissions-view",
  },
  {
    name: "updates a wrapper permission",
    invoke: () =>
      putWrapperPermission(
        "store-a",
        "client-a",
        "wrapper-a",
        "resource-a",
        {
          mode: { kind: "unlocked" },
          reason: "synthetic test permission",
          writable: true,
        },
      ),
    method: "PUT",
    pathname:
      "/v1/admin/stores/store-a/wrapper-permissions/client-a/wrapper-a/resource-a",
    body: {
      mode: { kind: "unlocked" },
      reason: "synthetic test permission",
      writable: true,
    },
  },
  {
    name: "deletes a wrapper permission",
    invoke: () =>
      deleteWrapperPermission(
        "store-a",
        "client-a",
        "wrapper-a",
        "resource-a",
      ),
    method: "DELETE",
    pathname:
      "/v1/admin/stores/store-a/wrapper-permissions/client-a/wrapper-a/resource-a",
  },
];

afterEach(() => {
  vi.restoreAllMocks();
  _resetCsrfTokenForTesting();
});

describe("typed endpoint wrappers", () => {
  it.each(endpointCases)("$name", async ({ invoke, method, pathname, body }) => {
    const responseBody = { marker: pathname };
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify(responseBody), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }),
    );

    await expect(invoke()).resolves.toEqual(responseBody);

    expect(fetchSpy).toHaveBeenCalledTimes(1);
    const request = fetchSpy.mock.calls[0]![0] as Request;
    const url = new URL(request.url);
    expect(request.method).toBe(method);
    expect(url.origin).toBe("https://underwrap.test");
    expect(url.pathname).toBe(pathname);
    expect(url.search).toBe("");
    expect(request.credentials).toBe("include");
    if (body === undefined) {
      expect(request.body).toBeNull();
    } else {
      await expect(request.clone().json()).resolves.toEqual(body);
      expect(request.headers.get("Content-Type")).toBe("application/json");
    }
  });
});
