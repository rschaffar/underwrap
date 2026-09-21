import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link, useRouter, useRouterState } from "@tanstack/react-router";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import { Input } from "@/components/ui/input";
import { usePageIntent } from "@/hooks/use-page-intent";
import {
  type AdminClientInstanceListItem,
  type AdminClientInstancesListResponse,
  type AdminStoresListResponse,
  fetchClientInstances,
  fetchStores,
} from "@/api/client";
import { useDocumentTitle } from "@/hooks/use-document-title";
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
  const router = useRouter();
  const search = useRouterState({ select: (state) => state.location.search });
  const mappedStore =
    typeof search.mappedStore === "string" ? search.mappedStore : "";
  const exactClient = typeof search.client === "string" ? search.client : "";
  const [query, setQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<ClientStatusFilter>("all");
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<AdminClientInstanceListItem | null>(
    null,
  );
  const [settingStatus, setSettingStatus] = useState<{
    instance: AdminClientInstanceListItem;
    nextStatus: "active" | "disabled";
  } | null>(null);
  const [deleting, setDeleting] = useState<AdminClientInstanceListItem | null>(
    null,
  );

  useDocumentTitle("underwrap — client instances");

  const instancesQuery = useQuery<AdminClientInstancesListResponse>({
    queryKey: ["admin", "client-instances"],
    queryFn: fetchClientInstances,
  });
  const storesQuery = useQuery<AdminStoresListResponse>({
    queryKey: ["admin", "stores"],
    queryFn: fetchStores,
  });

  const intentStoreIsValid =
    storesQuery.isSuccess &&
    typeof search.store === "string" &&
    storesQuery.data.items.some((store) => store.id === search.store);
  usePageIntent("create", intentStoreIsValid, (value) => {
    if (value === "client") setCreating(true);
  });
  const items = useMemo(
    () =>
      (instancesQuery.data?.items ?? []).filter(
        (item) =>
          statusMatches(item, statusFilter) &&
          (!mappedStore || item.current_store_id === mappedStore) &&
          (!exactClient || item.id === exactClient) &&
          [item.id, item.display_name, item.current_store_name].some((value) =>
            value.toLowerCase().includes(query.trim().toLowerCase()),
          ),
      ),
    [instancesQuery.data, statusFilter, mappedStore, exactClient, query],
  );
  const setMappedStore = (value: string) =>
    void router.navigate({
      to: "/client-instances",
      hash: true,
      state: true,
      search: (previous) => ({
        ...previous,
        mappedStore: value || undefined,
        client: undefined,
      }),
      replace: true,
    });

  return (
    <div className="space-y-6">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold text-(--color-fg)">
            Client Instances
          </h2>
          <p className="mt-1 text-sm text-(--color-muted)">
            Runtime client identities registered with this backend. Generate a
            credential on the client host with <code>underwrap-keygen</code> and
            paste the cert PEM below to register it. Disabled instances stay
            visible here for management and auditability.
          </p>
        </div>
        <div className="flex items-end gap-3">
          <div className="space-y-1">
            <Label htmlFor="status-filter">Status filter</Label>
            <Select
              id="status-filter"
              className="w-auto"
              value={statusFilter}
              onChange={(e) =>
                setStatusFilter(e.target.value as ClientStatusFilter)
              }
            >
              <option value="all">All</option>
              <option value="active">Active</option>
              <option value="disabled">Disabled</option>
            </Select>
          </div>
          <Button
            type="button"
            disabled={
              storesQuery.isPending ||
              storesQuery.isError ||
              storesQuery.data?.items.length === 0
            }
            title={
              storesQuery.isError
                ? "The store list failed to load — reload to create instances."
                : storesQuery.isPending
                  ? "Loading stores…"
                  : undefined
            }
            onClick={() => setCreating(true)}
          >
            Create client instance
          </Button>
        </div>
      </header>

      <div className="flex flex-wrap items-end gap-3">
        <div className="min-w-64 flex-1 space-y-1">
          <Label htmlFor="client-search">Filter client metadata</Label>
          <Input
            id="client-search"
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Client name, ID or store name"
          />
        </div>
        <div className="space-y-1">
          <Label htmlFor="mapped-store-filter">Mapped store filter</Label>
          <Select
            id="mapped-store-filter"
            value={mappedStore}
            onChange={(event) => setMappedStore(event.target.value)}
          >
            <option value="">All stores</option>
            {mappedStore &&
            !storesQuery.data?.items.some(
              (store) => store.id === mappedStore,
            ) ? (
              <option value={mappedStore}>Unavailable store filter</option>
            ) : null}
            {storesQuery.data?.items.map((store) => (
              <option key={store.id} value={store.id}>
                {store.name}
              </option>
            ))}
          </Select>
        </div>
        {exactClient ? (
          <Button
            variant="ghost"
            onClick={() =>
              void router.navigate({
                to: "/client-instances",
                hash: true,
                state: true,
                search: (previous) => ({ ...previous, client: undefined }),
                replace: true,
              })
            }
          >
            Clear client filter
          </Button>
        ) : null}
      </div>
      <p className="text-xs text-(--color-muted)">
        This is the global inventory; selected store context does not filter it
        automatically. {items.length} matching clients.
      </p>
      <p className="text-sm text-(--color-muted)">
        Client-wide access controls are beneath each instance's column heading
        on Wrapper permissions. Use the link in its Store column to open the
        mapped store. Individual permission rows stay unchanged by overrides.
      </p>
      {storesQuery.isSuccess && storesQuery.data.items.length === 0 ? (
        <Alert variant="warning">
          Create a store before enrollment.{" "}
          <Link className="underline" to="/stores" search={{ create: "store" }}>
            Create store
          </Link>
          .
        </Alert>
      ) : null}

      {storesQuery.isError ? (
        <Alert>
          The store list could not be loaded (
          {describeApiError(storesQuery.error)}). Creating or editing client
          instances is unavailable until it loads.
        </Alert>
      ) : null}

      {instancesQuery.isLoading ? (
        <p className="text-sm text-(--color-muted)">Loading…</p>
      ) : instancesQuery.error ? (
        <Alert>{describeApiError(instancesQuery.error)}</Alert>
      ) : items.length === 0 ? (
        <p className="text-sm text-(--color-muted)">
          No client instances match the current filter.
        </p>
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
                      <div className="font-medium text-(--color-fg)">
                        {item.display_name}
                      </div>
                      <div className="text-xs text-(--color-muted)">
                        {item.id}
                      </div>
                    </td>
                    <td className="px-4 py-3 text-(--color-muted)">
                      <Link
                        className="underline"
                        to="/secrets"
                        search={{ store: item.current_store_id }}
                      >
                        {item.current_store_name}
                      </Link>
                      <div>
                        <Link
                          className="text-xs underline"
                          to="/wrappers"
                          search={{ store: item.current_store_id }}
                        >
                          Wrapper permissions
                        </Link>
                      </div>
                      <div className="text-xs">{item.current_store_id}</div>
                    </td>
                    <td className="px-4 py-3">
                      <StatusBadge status={item.status} />{" "}
                      <AutoCreateBadge
                        enabled={item.auto_create_wrapper_assets ?? false}
                      />
                    </td>
                    <td className="px-4 py-3 text-(--color-muted)">
                      {formatEpochAbsolute(item.last_seen_at)}
                    </td>
                    <td className="px-4 py-3 text-right">
                      <div className="inline-flex gap-2">
                        <Button
                          variant="ghost"
                          size="sm"
                          disabled={
                            storesQuery.isPending || storesQuery.isError
                          }
                          title={
                            storesQuery.isError
                              ? "The store list failed to load — reload to edit instances."
                              : storesQuery.isPending
                                ? "Loading stores…"
                                : undefined
                          }
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
                              nextStatus:
                                item.status === "active"
                                  ? "disabled"
                                  : "active",
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
          initialStoreId={
            typeof search.store === "string" ? search.store : undefined
          }
          onClose={() => setCreating(false)}
        />
      ) : null}

      {editing && storesQuery.data ? (
        <EditClientInstanceDialog
          instance={editing}
          stores={storesQuery.data.items}
          onClose={() => setEditing(null)}
        />
      ) : null}

      {settingStatus ? (
        <SetStatusDialog
          instance={settingStatus.instance}
          nextStatus={settingStatus.nextStatus}
          onClose={() => setSettingStatus(null)}
        />
      ) : null}

      {deleting ? (
        <DeleteClientInstanceDialog
          instance={deleting}
          onClose={() => setDeleting(null)}
        />
      ) : null}
    </div>
  );
}
