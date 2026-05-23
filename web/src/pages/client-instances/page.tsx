import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import {
  type AdminClientInstanceListItem,
  type AdminClientInstancesListResponse,
  type AdminStoresListResponse,
  fetchClientInstances,
  fetchStores,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";
import { formatEpochAbsolute } from "@/lib/epoch";

import { StatusBadge, AutoCreateBadge } from "./badges";
import { CreateClientInstanceDialog } from "./dialogs/create";
import { EditClientInstanceDialog } from "./dialogs/edit";
import { SetStatusDialog } from "./dialogs/set-status";
import { DeleteClientInstanceDialog } from "./dialogs/delete";

type ClientStatusFilter = "all" | "active" | "disabled";

function statusMatches(
  item: AdminClientInstancesListResponse["items"][number],
  filter: ClientStatusFilter,
): boolean {
  return filter === "all" || item.status === filter;
}

export function ClientInstancesPage() {
  const qc = useQueryClient();
  const [statusFilter, setStatusFilter] = useState<ClientStatusFilter>("all");
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] =
    useState<AdminClientInstanceListItem | null>(null);
  const [settingStatus, setSettingStatus] = useState<{
    instance: AdminClientInstanceListItem;
    nextStatus: "active" | "disabled";
  } | null>(null);
  const [deleting, setDeleting] =
    useState<AdminClientInstanceListItem | null>(null);

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
            <Select
              id="status-filter"
              className="w-auto"
              value={statusFilter}
              onChange={(e) => setStatusFilter(e.target.value as ClientStatusFilter)}
            >
              <option value="all">All</option>
              <option value="active">Active</option>
              <option value="disabled">Disabled</option>
            </Select>
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
                      <AutoCreateBadge enabled={item.auto_create_wrapper_assets ?? false} />
                    </td>
                    <td className="px-4 py-3 text-(--color-muted)">{formatEpochAbsolute(item.last_seen_at)}</td>
                    <td className="px-4 py-3 text-right">
                      <div className="inline-flex gap-2">
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => setEditing(item)}
                        >
                          Edit
                        </Button>
                        <Button
                          variant={canDisable ? "destructive" : "ghost"}
                          size="sm"
                          disabled={!canDisable && !canEnable}
                          onClick={() =>
                            setSettingStatus({
                              instance: item,
                              nextStatus: item.status === "active" ? "disabled" : "active",
                            })
                          }
                        >
                          {item.status === "active" ? "Disable" : "Re-enable"}
                        </Button>
                        <Button
                          variant="destructive"
                          size="sm"
                          onClick={() => setDeleting(item)}
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
