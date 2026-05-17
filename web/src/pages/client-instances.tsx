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
import { Label } from "@/components/ui/label";
import {
  type AdminClientInstanceDetailResponse,
  type AdminClientInstancesListResponse,
  type AdminStoresListResponse,
  fetchClientInstances,
  fetchStores,
  putClientInstanceUpdate,
  type WhoamiResponse,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

type ClientStatusFilter = "all" | "active" | "disabled";

export function canManageClientInstances(p: WhoamiResponse | null): boolean {
  return !!(p && p.authenticated && p.role === "admin");
}

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

export function ClientInstancesPage({
  principal,
}: {
  principal: WhoamiResponse | null;
}) {
  const qc = useQueryClient();
  const isAdmin = canManageClientInstances(principal);
  const [statusFilter, setStatusFilter] = useState<ClientStatusFilter>("all");
  const [remapping, setRemapping] = useState<AdminClientInstanceDetailResponse | null>(null);
  const [settingStatus, setSettingStatus] = useState<{
    instance: AdminClientInstanceDetailResponse;
    nextStatus: "active" | "disabled";
  } | null>(null);

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
            Runtime client identities enrolled into this backend. Remaps apply on the next runtime
            request; disabled instances stay visible here for management and auditability.
          </p>
        </div>
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
                const canRemap = isAdmin && item.actions.can_remap_store;
                const canDisable = isAdmin && item.actions.can_disable;
                const canEnable = isAdmin && item.actions.can_enable;
                return (
                  <tr
                    key={item.id}
                    data-testid={`client-instance-row-${item.id}`}
                    className="border-t border-(--color-line) align-middle"
                  >
                    <td className="px-4 py-3">
                      <div className="font-medium text-(--color-fg)">{item.display_name}</div>
                      <div className="text-xs text-(--color-muted)">
                        {item.hostname_hint ?? "unknown-host"} · instance {item.instance_name}
                      </div>
                    </td>
                    <td className="px-4 py-3 text-(--color-muted)">
                      <div>{item.current_store_name}</div>
                      <div className="text-xs">{item.current_store_id}</div>
                    </td>
                    <td className="px-4 py-3">
                      <StatusBadge status={item.status} />
                    </td>
                    <td className="px-4 py-3 text-(--color-muted)">{formatEpoch(item.last_seen_at)}</td>
                    <td className="px-4 py-3 text-right">
                      <div className="inline-flex gap-2">
                        <Button
                          variant="ghost"
                          size="sm"
                          disabled={!canRemap || (storesQuery.data?.items.length ?? 0) < 2}
                          title={
                            !isAdmin
                              ? "Only admins may manage client instances."
                              : (storesQuery.data?.items.length ?? 0) < 2
                                ? "Create another store before remapping client instances."
                                : undefined
                          }
                          onClick={() => setRemapping(toDetail(item))}
                        >
                          Remap store
                        </Button>
                        <Button
                          variant={canDisable ? "destructive" : "ghost"}
                          size="sm"
                          disabled={!canDisable && !canEnable}
                          title={!isAdmin ? "Only admins may manage client instances." : undefined}
                          onClick={() =>
                            setSettingStatus({
                              instance: toDetail(item),
                              nextStatus: item.status === "active" ? "disabled" : "active",
                            })
                          }
                        >
                          {item.status === "active" ? "Disable" : "Re-enable"}
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

      {isAdmin && remapping ? storesQuery.data ? (
        <RemapStoreDialog
          instance={remapping}
          stores={storesQuery.data.items}
          onClose={() => setRemapping(null)}
          onSuccess={async () => {
            await qc.invalidateQueries({ queryKey: ["admin", "client-instances"] });
            setRemapping(null);
          }}
        />
      ) : null : null}

      {isAdmin && settingStatus ? (
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
    </div>
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

function RemapStoreDialog({
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
  const [storeId, setStoreId] = useState(instance.current_store_id);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: (current_store_id: string) =>
      putClientInstanceUpdate(instance.id, { current_store_id }),
    onSuccess: async () => {
      setSubmitError(null);
      await onSuccess();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  const options = stores.filter((store) => store.id !== instance.current_store_id);

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Remap store: {instance.display_name}</DialogTitle>
          <DialogDescription>
            Select a new target store. This takes effect on the next runtime request for this client
            instance.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <div className="rounded-md border border-(--color-line) bg-(--color-surface-1) px-3 py-2 text-xs text-(--color-muted)">
            Current store: <strong>{instance.current_store_name}</strong>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="target-store">New target store</Label>
            <select
              id="target-store"
              value={storeId}
              onChange={(e) => setStoreId(e.target.value)}
              className="flex h-9 w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 text-sm text-(--color-fg)"
            >
              <option value={instance.current_store_id} disabled>
                Choose a different store
              </option>
              {options.map((store) => (
                <option key={store.id} value={store.id}>
                  {store.name}
                </option>
              ))}
            </select>
          </div>
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
            disabled={storeId === instance.current_store_id || mut.isPending}
            onClick={() => mut.mutate(storeId)}
          >
            {mut.isPending ? "Remapping…" : "Remap now"}
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
