import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  type AdminClientInstanceCreateRequest,
  type AdminClientInstanceDetailResponse,
  type AdminClientInstancesListResponse,
  type AdminStoresListResponse,
  deleteClientInstance,
  fetchClientInstances,
  fetchStores,
  postClientInstanceCreate,
  putClientInstanceUpdate,
  type WhoamiResponse,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

type ClientStatusFilter = "all" | "active" | "disabled";

function formatEpoch(epoch?: bigint | number): string {
  if (epoch === undefined) return "Never";
  const n = typeof epoch === "bigint" ? Number(epoch) : epoch;
  if (!n) return "Never";
  return new Date(n * 1000).toLocaleString();
}

function toDetail(
  item: AdminClientInstancesListResponse["items"][number],
): AdminClientInstanceDetailResponse {
  return {
    ...item,
    current_credential_issued_at: undefined,
  };
}

function statusMatches(
  item: AdminClientInstancesListResponse["items"][number],
  filter: ClientStatusFilter,
): boolean {
  return filter === "all" || item.status === filter;
}

export function ClientInstancesPage(_props: {
  principal?: WhoamiResponse | null;
} = {}) {
  const qc = useQueryClient();
  const [statusFilter, setStatusFilter] = useState<ClientStatusFilter>("all");
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] =
    useState<AdminClientInstanceDetailResponse | null>(null);
  const [settingStatus, setSettingStatus] = useState<{
    instance: AdminClientInstanceDetailResponse;
    nextStatus: "active" | "disabled";
  } | null>(null);
  const [deleting, setDeleting] =
    useState<AdminClientInstanceDetailResponse | null>(null);

  useEffect(() => {
    document.title = "underwrap — client instances";
  }, []);

  const instancesQuery = useQuery<AdminClientInstancesListResponse>({
    queryKey: ["admin", "client-instances"],
    queryFn: fetchClientInstances,
  });
  const storesQuery = useQuery<AdminStoresListResponse>({
    queryKey: ["admin", "stores"],
    queryFn: fetchStores,
  });

  const items = useMemo(
    () => (instancesQuery.data?.items ?? []).filter((item) => statusMatches(item, statusFilter)),
    [instancesQuery.data, statusFilter],
  );

  return (
    <div className="space-y-6">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold text-(--color-fg)">Client Instances</h2>
          <p className="mt-1 text-sm text-(--color-muted)">
            Runtime client identities registered with this backend. Generate a credential
            on the client host with <code>underwrap-keygen</code> and paste the cert PEM
            below to register it. Edits apply on the next runtime request; disabled
            instances stay visible here for management and auditability.
          </p>
        </div>
        <div className="flex items-end gap-3">
          <div className="space-y-1">
            <Label htmlFor="status-filter">Status filter</Label>
            <select
              id="status-filter"
              className="flex h-9 rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 text-sm text-(--color-fg)"
              value={statusFilter}
              onChange={(e) => setStatusFilter(e.target.value as ClientStatusFilter)}
            >
              <option value="all">All</option>
              <option value="active">Active</option>
              <option value="disabled">Disabled</option>
            </select>
          </div>
          <Button
            type="button"
            onClick={() => setCreating(true)}
          >
            Create client instance
          </Button>
        </div>
      </header>

      {instancesQuery.isLoading ? (
        <p className="text-sm text-(--color-muted)">Loading…</p>
      ) : instancesQuery.error ? (
        <p role="alert" className="text-sm text-rose-300">
          {describeApiError(instancesQuery.error)}
        </p>
      ) : items.length === 0 ? (
        <p className="text-sm text-(--color-muted)">No client instances match the current filter.</p>
      ) : (
        <div className="overflow-x-auto rounded-xl border border-(--color-line)">
          <table className="w-full text-left text-sm">
            <thead className="bg-(--color-surface-0)/60 text-xs uppercase tracking-wide text-(--color-muted)">
              <tr>
                <th className="px-4 py-2">Instance</th>
                <th className="px-4 py-2">Store</th>
                <th className="px-4 py-2">Status</th>
                <th className="px-4 py-2">Last seen</th>
                <th className="px-4 py-2 text-right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {items.map((item) => {
                const canDisable = item.actions.can_disable;
                const canEnable = item.actions.can_enable;
                const canDelete = item.actions.can_delete;
                return (
                  <tr
                    key={item.id}
                    data-testid={`client-instance-row-${item.id}`}
                    className="border-t border-(--color-line) align-middle"
                  >
                    <td className="px-4 py-3">
                      <div className="font-medium text-(--color-fg)">{item.display_name}</div>
                    </td>
                    <td className="px-4 py-3 text-(--color-muted)">
                      <div>{item.current_store_name}</div>
                      <div className="text-xs">{item.current_store_id}</div>
                    </td>
                    <td className="px-4 py-3">
                      <StatusBadge status={item.status} />{" "}
                      <AutoCreateBadge enabled={item.auto_create_wrapper_assets} />
                    </td>
                    <td className="px-4 py-3 text-(--color-muted)">{formatEpoch(item.last_seen_at)}</td>
                    <td className="px-4 py-3 text-right">
                      <div className="inline-flex gap-2">
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => setEditing(toDetail(item))}
                        >
                          Edit
                        </Button>
                        <Button
                          variant={canDisable ? "destructive" : "ghost"}
                          size="sm"
                          disabled={!canDisable && !canEnable}
                          onClick={() =>
                            setSettingStatus({
                              instance: toDetail(item),
                              nextStatus: item.status === "active" ? "disabled" : "active",
                            })
                          }
                        >
                          {item.status === "active" ? "Disable" : "Re-enable"}
                        </Button>
                        <Button
                          variant="destructive"
                          size="sm"
                          disabled={!canDelete}
                          onClick={() => setDeleting(toDetail(item))}
                        >
                          Delete
                        </Button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {creating && storesQuery.data ? (
        <CreateClientInstanceDialog
          stores={storesQuery.data.items}
          onClose={() => setCreating(false)}
          onSuccess={async () => {
            await qc.invalidateQueries({ queryKey: ["admin", "client-instances"] });
            setCreating(false);
          }}
        />
      ) : null}

      {editing && storesQuery.data ? (
        <EditClientInstanceDialog
          instance={editing}
          stores={storesQuery.data.items}
          onClose={() => setEditing(null)}
          onSuccess={async () => {
            await qc.invalidateQueries({ queryKey: ["admin", "client-instances"] });
            setEditing(null);
          }}
        />
      ) : null}

      {settingStatus ? (
        <SetStatusDialog
          instance={settingStatus.instance}
          nextStatus={settingStatus.nextStatus}
          onClose={() => setSettingStatus(null)}
          onSuccess={async () => {
            await qc.invalidateQueries({ queryKey: ["admin", "client-instances"] });
            setSettingStatus(null);
          }}
        />
      ) : null}

      {deleting ? (
        <DeleteClientInstanceDialog
          instance={deleting}
          onClose={() => setDeleting(null)}
          onSuccess={async () => {
            await qc.invalidateQueries({ queryKey: ["admin", "client-instances"] });
            setDeleting(null);
          }}
        />
      ) : null}
    </div>
  );
}

function CreateClientInstanceDialog({
  stores,
  onClose,
  onSuccess,
}: {
  stores: AdminStoresListResponse["items"];
  onClose: () => void;
  onSuccess: () => Promise<void>;
}) {
  const [displayName, setDisplayName] = useState("");
  const [storeId, setStoreId] = useState(stores[0]?.id ?? "");
  const [status, setStatus] = useState<"active" | "disabled">("active");
  const [certPem, setCertPem] = useState("");
  const [autoCreate, setAutoCreate] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const mut = useMutation({
    mutationFn: (body: AdminClientInstanceCreateRequest) =>
      postClientInstanceCreate(body),
    onSuccess: async () => {
      setSubmitError(null);
      await onSuccess();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  const trimmedCert = certPem.trim();
  const canSubmit =
    displayName.trim() !== "" &&
    storeId !== "" &&
    trimmedCert !== "" &&
    !mut.isPending;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create client instance</DialogTitle>
          <DialogDescription>
            Generate a credential on the client host with
            <code className="mx-1">underwrap-keygen</code>
            and paste the certificate PEM below. The private key never leaves the
            client host; only the certificate is registered here.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="create-display-name">Display name</Label>
            <Input
              id="create-display-name"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder="laptop-work"
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="create-store">Target store</Label>
            <select
              id="create-store"
              value={storeId}
              onChange={(e) => setStoreId(e.target.value)}
              className="flex h-9 w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 text-sm text-(--color-fg)"
            >
              {stores.map((store) => (
                <option key={store.id} value={store.id}>
                  {store.name}
                </option>
              ))}
            </select>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="create-status">Status</Label>
            <select
              id="create-status"
              value={status}
              onChange={(e) => setStatus(e.target.value as "active" | "disabled")}
              className="flex h-9 w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 text-sm text-(--color-fg)"
            >
              <option value="active">Active</option>
              <option value="disabled">Disabled</option>
            </select>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="create-cert-pem">Certificate PEM</Label>
            <textarea
              id="create-cert-pem"
              value={certPem}
              onChange={(e) => setCertPem(e.target.value)}
              rows={8}
              spellCheck={false}
              placeholder={"-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----"}
              className="flex w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 font-mono text-xs text-(--color-fg)"
            />
          </div>
          <label className="flex items-start gap-2 text-xs text-(--color-muted)">
            <input
              type="checkbox"
              data-testid="create-auto-create"
              checked={autoCreate}
              onChange={(e) => setAutoCreate(e.target.checked)}
              className="mt-0.5"
            />
            <span>
              <strong className="text-(--color-fg)">
                Auto-create wrapper assets on register
              </strong>
              <br />
              When on, this instance's wrappers may add new resources
              and wrapper/resource pairs on startup. When off, the
              backend refuses to create them and logs which entries it
              expected.
            </span>
          </label>
          {submitError ? (
            <p
              role="alert"
              className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
            >
              {submitError}
            </p>
          ) : null}
        </div>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            disabled={!canSubmit}
            onClick={() =>
              mut.mutate({
                display_name: displayName.trim(),
                current_store_id: storeId,
                status,
                cert_pem: trimmedCert,
                auto_create_wrapper_assets: autoCreate ? true : undefined,
              })
            }
          >
            {mut.isPending ? "Creating…" : "Create"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function EditClientInstanceDialog({
  instance,
  stores,
  onClose,
  onSuccess,
}: {
  instance: AdminClientInstanceDetailResponse;
  stores: AdminStoresListResponse["items"];
  onClose: () => void;
  onSuccess: () => Promise<void>;
}) {
  const [displayName, setDisplayName] = useState(instance.display_name);
  const [storeId, setStoreId] = useState(instance.current_store_id);
  const [certPem, setCertPem] = useState("");
  const [autoCreate, setAutoCreate] = useState(
    instance.auto_create_wrapper_assets,
  );
  const [submitError, setSubmitError] = useState<string | null>(null);

  const mut = useMutation({
    mutationFn: (body: {
      display_name?: string;
      current_store_id?: string;
      cert_pem?: string;
      auto_create_wrapper_assets?: boolean;
    }) => putClientInstanceUpdate(instance.id, body),
    onSuccess: async () => {
      setSubmitError(null);
      await onSuccess();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  const trimmedDisplayName = displayName.trim();
  const trimmedCert = certPem.trim();
  const displayNameChanged =
    trimmedDisplayName !== "" && trimmedDisplayName !== instance.display_name;
  const storeChanged = storeId !== instance.current_store_id;
  const certChanged = trimmedCert !== "";
  const autoCreateChanged = autoCreate !== instance.auto_create_wrapper_assets;
  const canSubmit =
    trimmedDisplayName !== "" &&
    (displayNameChanged || storeChanged || certChanged || autoCreateChanged) &&
    !mut.isPending;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit: {instance.display_name}</DialogTitle>
          <DialogDescription>
            Rename, remap the target store, or replace the registered
            certificate. Changes take effect on the next runtime request.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <div className="rounded-md border border-(--color-line) bg-(--color-surface-1) px-3 py-2 text-xs text-(--color-muted)">
            Current fingerprint:{" "}
            <code>{instance.current_credential_fingerprint}</code>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="edit-display-name">Display name</Label>
            <Input
              id="edit-display-name"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder="laptop-work"
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="edit-store">Target store</Label>
            <select
              id="edit-store"
              value={storeId}
              onChange={(e) => setStoreId(e.target.value)}
              className="flex h-9 w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 text-sm text-(--color-fg)"
            >
              {stores.map((store) => (
                <option key={store.id} value={store.id}>
                  {store.name}
                  {store.id === instance.current_store_id ? " (current)" : ""}
                </option>
              ))}
            </select>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="edit-cert-pem">New certificate PEM (optional)</Label>
            <textarea
              id="edit-cert-pem"
              value={certPem}
              onChange={(e) => setCertPem(e.target.value)}
              rows={8}
              spellCheck={false}
              placeholder={"-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----"}
              className="flex w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 font-mono text-xs text-(--color-fg)"
            />
            <p className="text-xs text-(--color-muted)">
              Generate a new credential on the client host with{" "}
              <code>underwrap-keygen</code> and paste the cert PEM to rotate.
              Leave blank to keep the existing cert.
            </p>
          </div>
          <label className="flex items-start gap-2 text-xs text-(--color-muted)">
            <input
              type="checkbox"
              data-testid="edit-auto-create"
              checked={autoCreate}
              onChange={(e) => setAutoCreate(e.target.checked)}
              className="mt-0.5"
            />
            <span>
              <strong className="text-(--color-fg)">
                Auto-create wrapper assets on register
              </strong>
              <br />
              When on, this instance's wrappers may add new resources
              and wrapper/resource pairs on startup.
            </span>
          </label>
          {submitError ? (
            <p
              role="alert"
              className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
            >
              {submitError}
            </p>
          ) : null}
        </div>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            disabled={!canSubmit}
            onClick={() =>
              mut.mutate({
                ...(displayNameChanged ? { display_name: trimmedDisplayName } : {}),
                ...(storeChanged ? { current_store_id: storeId } : {}),
                ...(certChanged ? { cert_pem: trimmedCert } : {}),
                ...(autoCreateChanged
                  ? { auto_create_wrapper_assets: autoCreate }
                  : {}),
              })
            }
          >
            {mut.isPending ? "Saving…" : "Save"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function StatusBadge({ status }: { status: "active" | "disabled" }) {
  const active = status === "active";
  return (
    <span
      className={
        "inline-flex items-center rounded-md border px-2 py-0.5 text-xs " +
        (active
          ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-300"
          : "border-amber-500/30 bg-amber-500/10 text-amber-300")
      }
    >
      {status}
    </span>
  );
}

function AutoCreateBadge({ enabled }: { enabled: boolean }) {
  return (
    <span
      className={
        "inline-flex items-center rounded-md border px-2 py-0.5 text-xs " +
        (enabled
          ? "border-sky-500/30 bg-sky-500/10 text-sky-300"
          : "border-(--color-line) bg-(--color-surface-1) text-(--color-muted)")
      }
      title={
        enabled
          ? "This instance's wrappers may auto-create resource and wrapper/resource-pair records on startup."
          : "This instance's wrappers cannot auto-create resource or pair records. An admin must enable the flag or create them manually."
      }
      data-testid={`auto-create-badge-${enabled ? "on" : "off"}`}
    >
      {enabled ? "auto" : "manual"}
    </span>
  );
}

function DeleteClientInstanceDialog({
  instance,
  onClose,
  onSuccess,
}: {
  instance: AdminClientInstanceDetailResponse;
  onClose: () => void;
  onSuccess: () => Promise<void>;
}) {
  const [submitError, setSubmitError] = useState<string | null>(null);

  const mut = useMutation({
    mutationFn: () => deleteClientInstance(instance.id),
    onSuccess: async () => {
      setSubmitError(null);
      await onSuccess();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  const canDelete = !mut.isPending;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete client instance: {instance.display_name}</DialogTitle>
          <DialogDescription>
            This removes the client instance record permanently. The
            client's registered certificate will no longer be accepted
            for runtime requests.
          </DialogDescription>
        </DialogHeader>
        {submitError ? (
          <p
            role="alert"
            className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
          >
            {submitError}
          </p>
        ) : null}
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            variant="destructive"
            disabled={!canDelete}
            onClick={() => mut.mutate()}
          >
            {mut.isPending ? "Deleting…" : "Delete"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function SetStatusDialog({
  instance,
  nextStatus,
  onClose,
  onSuccess,
}: {
  instance: AdminClientInstanceDetailResponse;
  nextStatus: "active" | "disabled";
  onClose: () => void;
  onSuccess: () => Promise<void>;
}) {
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: (status: "active" | "disabled") => putClientInstanceUpdate(instance.id, { status }),
    onSuccess: async () => {
      setSubmitError(null);
      await onSuccess();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {nextStatus === "disabled" ? "Disable" : "Re-enable"}: {instance.display_name}
          </DialogTitle>
          <DialogDescription>
            {nextStatus === "disabled"
              ? "Disabling this instance denies new runtime requests immediately. Existing list visibility is retained so operators can still manage it."
              : "Re-enabling this instance allows new runtime requests again immediately."}
          </DialogDescription>
        </DialogHeader>
        {submitError ? (
          <p
            role="alert"
            className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
          >
            {submitError}
          </p>
        ) : null}
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            variant={nextStatus === "disabled" ? "destructive" : "default"}
            disabled={mut.isPending}
            onClick={() => mut.mutate(nextStatus)}
          >
            {mut.isPending
              ? nextStatus === "disabled"
                ? "Disabling…"
                : "Updating…"
              : nextStatus === "disabled"
                ? "Disable now"
                : "Re-enable now"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
