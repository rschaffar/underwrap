import { StrictMode, useState, type ReactNode } from "react";
import {
  act, fireEvent, render, screen, waitFor, within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "@/api/client";
import { withTestRouter } from "@/test-router";
import { SecretsPage } from "./secrets/page";
import { WrapperPermissionsPage } from "./wrapper-permissions/page";
import { StoresPage } from "./stores/page";
import { LibsecretTree } from "./secrets/libsecret/tree";
import { libsecretTreeKey } from "./secrets/libsecret/helpers";
import { CreateResourceDialog } from "./secrets/dialogs/create-resource";
import { RenameResourceDialog } from "./secrets/dialogs/rename-resource";
import { CreateCollectionDialog } from "./secrets/libsecret/dialogs/create-collection";
import { CreateItemDialog } from "./secrets/libsecret/dialogs/create-item";
import { SetAliasDialog } from "./secrets/libsecret/dialogs/set-alias";
import { CreateUserDialog } from "./users/dialogs/create";
import { EditUserDialog } from "./users/dialogs/edit";
import { ResetPasswordDialog } from "./users/dialogs/reset-password";
import { EditStoreDialog } from "./stores/dialogs/edit";
import { UnlockStoreDialog } from "./stores/dialogs/unlock";
import { CreatePairDialog } from "./wrapper-permissions/dialogs/create-pair";

vi.mock("@/api/client", async () => ({
  ...(await vi.importActual<typeof import("@/api/client")>("@/api/client")),
  fetchStores: vi.fn(),
  fetchClientInstances: vi.fn(),
  fetchSecretsView: vi.fn(),
  fetchWrapperPermissionsView: vi.fn(),
  fetchLibsecretTree: vi.fn(),
  postResourceCreate: vi.fn(),
  putResourceRename: vi.fn(),
  postLibsecretCollection: vi.fn(),
  postLibsecretItem: vi.fn(),
  putLibsecretAlias: vi.fn(),
  postUserCreate: vi.fn(),
  putUserUpdate: vi.fn(),
  postUserResetPassword: vi.fn(),
  putStoreUpdate: vi.fn(),
  postStoreUnlock: vi.fn(),
  postPairCreate: vi.fn(),
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}
const store: api.AdminStoreListItem = {
  id: "primary",
  name: "Primary",
  backend: "plain",
  lifecycle_state: "ready",
  usage: { mapped_client_instance_count: 0, has_secrets: false },
  actions: {
    can_update: true,
    can_migrate: true,
    can_rotate_key: false,
    can_unlock: false,
    can_lock: false,
    can_delete_now: true,
    can_force_delete: false,
  },
};
const operator = {
  id: "operator", username: "operator", created_at: 0, password_changed_at: 0,
};
const resource = {
  id: "resource", name: "Existing", created_at: 0,
  created_by: { kind: "operator" as const, id: "operator", username: "operator" },
};
const collection = {
  id: "collection", label: "Collection", created: 0, modified: 0, items: [],
};
const scope = { storeId: store.id, resourceId: resource.id };

const clients: QueryClient[] = [];
afterEach(() => {
  for (const qc of clients.splice(0)) qc.clear();
});
function harness(children: ReactNode) {
  window.history.replaceState({}, "", "/secrets?store=primary");
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  clients.push(qc);
  return {
    qc,
    ...render(
      <QueryClientProvider client={qc}>
        {withTestRouter(<StrictMode>{children}</StrictMode>, {
          "/users": <p>Navigation destination</p>,
        })}
      </QueryClientProvider>,
    ),
  };
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.fetchStores).mockResolvedValue({ items: [store] });
  vi.mocked(api.fetchClientInstances).mockResolvedValue({ items: [] });
  vi.mocked(api.fetchSecretsView).mockResolvedValue({ entries: [], resources: [resource] });
  vi.mocked(api.fetchWrapperPermissionsView).mockResolvedValue({ permissions: [], pairs: [], resources: [resource] });
  vi.mocked(api.fetchLibsecretTree).mockResolvedValue({ aliases: {}, collections: [collection] });
});

const cases = [
  {
    name: "resource create", request: api.postResourceCreate,
    dialog: (onClose: () => void) => (
      <CreateResourceDialog storeId={store.id} existingResources={[]} onClose={onClose} />
    ),
    fields: { "Resource name": "New resource" },
  },
  {
    name: "resource rename", request: api.putResourceRename,
    dialog: (onClose: () => void) => (
      <RenameResourceDialog {...scope} currentName="Existing" existingNames={["Existing"]} onClose={onClose} />
    ),
    fields: { "New name": "Renamed" },
  },
  {
    name: "collection create", request: api.postLibsecretCollection,
    dialog: (onClose: () => void) => (
      <CreateCollectionDialog {...scope} existingIds={[]} onClose={onClose} />
    ),
    fields: { "Label": "New collection" },
  },
  {
    name: "item create", request: api.postLibsecretItem,
    dialog: (onClose: () => void) => (
      <CreateItemDialog {...scope} collectionId={collection.id} existingIds={[]} onClose={onClose} />
    ),
    fields: { "Label": "New item", "Secret material": "synthetic-test-material" },
  },
  {
    name: "alias set", request: api.putLibsecretAlias,
    dialog: (onClose: () => void) => (
      <SetAliasDialog {...scope} collections={[collection]} existingAliases={[]} onClose={onClose} />
    ),
    fields: {},
  },
  {
    name: "user create", request: api.postUserCreate,
    dialog: (onClose: () => void) => <CreateUserDialog open onClose={onClose} />,
    fields: {
      "Username": "new-operator",
      "Password": "synthetic-test-password", "Confirm password": "synthetic-test-password",
    },
  },
  {
    name: "user edit", request: api.putUserUpdate,
    dialog: (onClose: () => void) => <EditUserDialog user={operator} onClose={onClose} />,
    fields: { "Username": "renamed-operator" },
  },
  {
    name: "password reset", request: api.postUserResetPassword,
    dialog: (onClose: () => void) => <ResetPasswordDialog user={operator} onClose={onClose} />,
    fields: {
      "New password": "synthetic-test-password", "Confirm new password": "synthetic-test-password",
    },
  },
  {
    name: "store edit", request: api.putStoreUpdate,
    dialog: (onClose: () => void) => <EditStoreDialog store={store} onClose={onClose} />,
    fields: { "Description": "New description" },
  },
  {
    name: "store unlock", request: api.postStoreUnlock,
    dialog: (onClose: () => void) => (
      <UnlockStoreDialog store={{ ...store, backend: "encrypted", lifecycle_state: "locked" }} onClose={onClose} />
    ),
    fields: { "Master key (hex)": "0".repeat(64) },
  },
  {
    name: "pair create", request: api.postPairCreate,
    dialog: (onClose: () => void) => (
      <CreatePairDialog storeId={store.id} existingResources={[resource]} onClose={onClose} />
    ),
    fields: { "Wrapper name": "new-wrapper" },
  },
];

function DraftOwner({ dialog, closed }: { dialog: (onClose: () => void) => ReactNode; closed: () => void }) {
  const [open, setOpen] = useState(true);
  return <>
    <Link to="/users">Navigate away</Link>
    <button onClick={() => setOpen(true)}>Open draft</button>
    {open ? dialog(() => { closed(); setOpen(false); }) : null}
  </>;
}

async function expectLocked(dialog: HTMLElement) {
  const user = userEvent.setup();
  const cancel = within(dialog).getByRole("button", { name: "Cancel" });
  const close = within(dialog).getByRole("button", { name: "Close" });
  expect(cancel).toBeDisabled();
  expect(close).toBeDisabled();
  await user.click(cancel);
  await user.click(close);
  await user.keyboard("{Escape}");
  fireEvent.pointerDown(document.body, { pointerType: "mouse", button: 0 });
  fireEvent.focusIn(document.body);
  expect(dialog).toBeInTheDocument();
  const unload = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(unload);
  expect(unload.defaultPrevented).toBe(true);
  // Bypass modal pointer shielding to check the route blocker itself.
  fireEvent.click(screen.getByText("Navigate away"));
  await waitFor(() => expect(window.location.pathname).toBe("/secrets"));
}

describe("form draft lifecycle", () => {
  it.each(cases)("guards $name through request and invalidation, then allows a fresh draft", async ({ request, dialog, fields }) => {
    const pendingRequest = deferred<never>();
    const invalidation = deferred<void>();
    vi.mocked(request).mockImplementation(() => pendingRequest.promise);
    const closed = vi.fn();
    const { qc } = harness(<DraftOwner dialog={dialog} closed={closed} />);
    vi.spyOn(qc, "invalidateQueries").mockReturnValue(invalidation.promise);
    const current = await screen.findByRole("dialog");
    for (const [label, value] of Object.entries(fields)) {
      fireEvent.change(within(current).getByLabelText(label, { exact: true }), { target: { value } });
    }
    fireEvent.submit(current.querySelector("form")!);
    await waitFor(() => expect(request).toHaveBeenCalledOnce());
    await expectLocked(current);
    // Attempt a reopen while pending. It must still be the same mounted draft.
    fireEvent.click(screen.getByText("Open draft"));
    expect(screen.getByRole("dialog")).toBe(current);
    expect(closed).not.toHaveBeenCalled();
    await act(async () => pendingRequest.resolve(undefined as never));
    await waitFor(() => expect(qc.invalidateQueries).toHaveBeenCalled());
    await expectLocked(current);
    expect(closed).not.toHaveBeenCalled();
    await act(async () => invalidation.resolve());
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(closed).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByText("Open draft"));
    const fresh = await screen.findByRole("dialog");
    expect(fresh).not.toBe(current);
    expect(within(fresh).getByRole("button", { name: "Cancel" })).toBeEnabled();
    await userEvent.setup().click(within(fresh).getByRole("button", { name: "Cancel" }));
    fireEvent.click(screen.getByText("Navigate away"));
    expect(await screen.findByText("Navigation destination")).toBeInTheDocument();
    expect(request).toHaveBeenCalledOnce();
  });

  it("retains a rejected Create resource draft on the real page, retries, and keeps a later draft after old success", async () => {
    const first = deferred<never>();
    const retry = deferred<never>();
    const invalidation = deferred<void>();
    vi.mocked(api.postResourceCreate).mockReturnValueOnce(first.promise).mockReturnValueOnce(retry.promise);
    const { qc } = harness(<SecretsPage />);
    const invalidate = vi.spyOn(qc, "invalidateQueries").mockReturnValue(invalidation.promise);
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Create resource" }));
    const draft = screen.getByRole("dialog", { name: "Create resource" });
    await user.type(within(draft).getByLabelText("Resource name"), "First draft");
    await user.click(within(draft).getByRole("button", { name: "Create" }));
    await waitFor(() => expect(api.postResourceCreate).toHaveBeenCalledOnce());
    expect(within(draft).getByRole("button", { name: "Cancel" })).toBeDisabled();
    await act(async () => first.reject(new api.ApiError(503, "synthetic_failure", "Synthetic request failure.")));
    expect(await within(draft).findByRole("alert")).toHaveTextContent("Synthetic request failure.");
    expect(within(draft).getByLabelText("Resource name")).toHaveValue("First draft");
    expect(within(draft).getByRole("button", { name: "Cancel" })).toBeEnabled();
    expect(invalidate).not.toHaveBeenCalled();
    await user.click(within(draft).getByRole("button", { name: "Create" }));
    await waitFor(() => expect(api.postResourceCreate).toHaveBeenCalledTimes(2));
    await act(async () => retry.resolve(undefined as never));
    await waitFor(() => expect(invalidate).toHaveBeenCalledTimes(2));
    await user.keyboard("{Escape}");
    expect(draft).toBeInTheDocument();
    expect(within(draft).getByRole("button", { name: "Cancel" })).toBeDisabled();
    await act(async () => invalidation.resolve());
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Create resource" }));
    const fresh = screen.getByRole("dialog");
    await user.type(within(fresh).getByLabelText("Resource name"), "Later draft");
    await act(async () => { await Promise.resolve(); });
    expect(within(fresh).getByLabelText("Resource name")).toHaveValue("Later draft");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(api.postResourceCreate).toHaveBeenNthCalledWith(2, store.id, { name: "First draft" });
  });

  describe.each(["request", "invalidation"] as const)("forced unmount during %s", (phase) => {
    it.each(["resource create", "resource rename", "pair create"])("does not let old %s completion close a fallback-store draft", async (kind) => {
      const request = kind === "resource create" ? api.postResourceCreate : kind === "resource rename" ? api.putResourceRename : api.postPairCreate;
      const pendingRequest = deferred<never>();
      const invalidation = deferred<void>();
      vi.mocked(request).mockImplementation(() => pendingRequest.promise);
      const secondary = { ...store, id: "secondary", name: "Secondary" };
      vi.mocked(api.fetchStores).mockResolvedValue({ items: [store, secondary] });
      const { qc } = harness(kind === "pair create" ? <WrapperPermissionsPage /> : <SecretsPage />);
      const invalidate = vi.spyOn(qc, "invalidateQueries").mockReturnValue(invalidation.promise);
      const openName = kind === "resource create" ? "Create resource" : kind === "resource rename" ? "Rename resource Existing" : "Create pair";
      const label = kind === "resource create" ? "Resource name" : kind === "resource rename" ? "New name" : "Wrapper name";
      const user = userEvent.setup();
      await user.click(await screen.findByRole("button", { name: openName }));
      const old = screen.getByRole("dialog");
      fireEvent.change(within(old).getByLabelText(label), { target: { value: "Old draft" } });
      fireEvent.submit(old.querySelector("form")!);
      await waitFor(() => expect(request).toHaveBeenCalledOnce());
      if (phase === "invalidation") {
        await act(async () => pendingRequest.resolve(undefined as never));
        await waitFor(() => expect(invalidate).toHaveBeenCalled());
      }
      // Authoritative list removal, not navigation: retain the page's parent setters.
      await act(async () => qc.setQueryData(["admin", "stores"], { items: [secondary] }));
      await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
      await user.click(screen.getByRole("button", { name: openName }));
      const fresh = screen.getByRole("dialog");
      fireEvent.change(within(fresh).getByLabelText(label), { target: { value: "Later draft" } });
      if (phase === "request") await act(async () => pendingRequest.resolve(undefined as never));
      await waitFor(() => expect(invalidate).toHaveBeenCalled());
      await act(async () => invalidation.resolve());
      expect(fresh).toBeInTheDocument();
      expect(within(fresh).getByLabelText(label)).toHaveValue("Later draft");
      expect(request).toHaveBeenCalledOnce();
      expect(invalidate).toHaveBeenCalledWith(expect.objectContaining({ queryKey: ["admin", kind === "pair create" ? "wrapper-permissions-view" : "secrets-view", store.id] }), { throwOnError: true });
      await user.click(within(fresh).getByRole("button", { name: "Cancel" }));
    });

    it("does not close a replacement libsecret draft after a refresh error/recovery", async () => {
      const pendingRequest = deferred<never>();
      const invalidation = deferred<void>();
      vi.mocked(api.postLibsecretCollection).mockReturnValue(pendingRequest.promise);
      const { qc } = harness(<LibsecretTree {...scope} enabled />);
      const invalidate = vi.spyOn(qc, "invalidateQueries").mockReturnValue(invalidation.promise);
      const user = userEvent.setup();
      await user.click(await screen.findByRole("button", { name: "Add libsecret collection" }));
      const old = screen.getByRole("dialog");
      fireEvent.change(within(old).getByLabelText("Label"), { target: { value: "Old collection" } });
      fireEvent.submit(old.querySelector("form")!);
      await waitFor(() => expect(api.postLibsecretCollection).toHaveBeenCalledOnce());
      if (phase === "invalidation") {
        await act(async () => pendingRequest.resolve(undefined as never));
        await waitFor(() => expect(invalidate).toHaveBeenCalled());
      }
      vi.mocked(api.fetchLibsecretTree).mockRejectedValueOnce(new api.ApiError(503, "synthetic_refresh", "Synthetic refresh failure."));
      await act(async () => { await qc.refetchQueries({ queryKey: libsecretTreeKey(store.id, resource.id) }); });
      expect(await screen.findByRole("alert")).toHaveTextContent("Synthetic refresh failure.");
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      await act(async () => { await qc.refetchQueries({ queryKey: libsecretTreeKey(store.id, resource.id) }); });
      const fresh = await screen.findByRole("dialog");
      expect(fresh).not.toBe(old);
      fireEvent.change(within(fresh).getByLabelText("Label"), { target: { value: "Later collection" } });
      if (phase === "request") await act(async () => pendingRequest.resolve(undefined as never));
      await waitFor(() => expect(invalidate).toHaveBeenCalled());
      await act(async () => invalidation.resolve());
      expect(fresh).toBeInTheDocument();
      expect(within(fresh).getByLabelText("Label")).toHaveValue("Later collection");
      expect(api.postLibsecretCollection).toHaveBeenCalledOnce();
    });

    it.each(["edit", "unlock"])("does not close another store's %s after inventory removal", async (kind) => {
      const request = kind === "edit" ? api.putStoreUpdate : api.postStoreUnlock;
      const pendingRequest = deferred<never>();
      const invalidation = deferred<void>();
      vi.mocked(request).mockImplementation(() => pendingRequest.promise);
      const first = kind === "edit" ? store : { ...store, backend: "encrypted" as const, lifecycle_state: "locked" as const, actions: { ...store.actions, can_unlock: true } };
      const second = { ...first, id: "secondary", name: "Secondary" };
      vi.mocked(api.fetchStores).mockResolvedValue({ items: [first, second] });
      const { qc } = harness(<StoresPage />);
      const invalidate = vi.spyOn(qc, "invalidateQueries").mockReturnValue(invalidation.promise);
      const user = userEvent.setup();
      const row = await screen.findByRole("row", { name: /Primary/ });
      await user.click(within(row).getByRole("button", { name: kind === "edit" ? "Edit" : "Unlock" }));
      const old = screen.getByRole("dialog");
      const label = kind === "edit" ? "Description" : "Master key (hex)";
      fireEvent.change(within(old).getByLabelText(label), { target: { value: kind === "edit" ? "Old description" : "0".repeat(64) } });
      fireEvent.submit(old.querySelector("form")!);
      await waitFor(() => expect(request).toHaveBeenCalledOnce());
      if (phase === "invalidation") {
        await act(async () => pendingRequest.resolve(undefined as never));
        await waitFor(() => expect(invalidate).toHaveBeenCalled());
      }
      await act(async () => qc.setQueryData(["admin", "stores"], { items: [second] }));
      await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
      await user.click(within(screen.getByRole("row", { name: /Secondary/ })).getByRole("button", { name: kind === "edit" ? "Edit" : "Unlock" }));
      const fresh = screen.getByRole("dialog");
      const value = kind === "edit" ? "Later description" : "1".repeat(64);
      fireEvent.change(within(fresh).getByLabelText(label), { target: { value } });
      if (phase === "request") await act(async () => pendingRequest.resolve(undefined as never));
      await waitFor(() => expect(invalidate).toHaveBeenCalled());
      await act(async () => invalidation.resolve());
      expect(fresh).toBeInTheDocument();
      expect(within(fresh).getByLabelText(label)).toHaveValue(value);
      expect(request).toHaveBeenCalledOnce();
    });
  });
});
