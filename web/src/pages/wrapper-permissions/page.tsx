import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Plus, FileText } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  StoreAvailability,
  storeIsAvailable,
} from "@/components/store-availability";
import { usePageIntent } from "@/hooks/use-page-intent";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { PolicyViewerDialog } from "@/components/policy-viewer-dialog";
import {
  type AdminStoresListResponse,
  type AdminWrapperPermissionsView,
  type WrapperPermission,
  type WrapperResourcePairRecord,
  fetchStores,
  fetchWrapperPermissionsView,
} from "@/api/client";
import { useDocumentTitle } from "@/hooks/use-document-title";
import { describeApiError } from "@/lib/error-message";
import { useStoreSelector } from "@/hooks/use-store-selector";
import { useClientInstanceDirectory } from "@/hooks/use-client-instance-directory";
import { useResourceCatalog } from "@/hooks/use-resource-catalog";

import {
  clientInstancesForStore,
  groupPairsByResource,
  permissionKey,
} from "./derivations";
import { ResourceGroupedPairsTable } from "./table";
import { CreatePairDialog } from "./dialogs/create-pair";
import { DeletePairDialog } from "./dialogs/delete-pair";
import { EditCellDialog } from "./dialogs/edit-cell";

export function WrapperPermissionsPage() {
  useDocumentTitle("underwrap — wrapper permissions");

  const stores = useQuery<AdminStoresListResponse>({
    queryKey: ["admin", "stores"],
    queryFn: fetchStores,
  });
  const { storeId, setStoreId } = useStoreSelector(
    stores.isSuccess ? stores.data.items : undefined,
  );
  const selectedStore = stores.data?.items.find(
    (store) => store.id === storeId,
  );
  const available = storeIsAvailable(selectedStore);

  // Aggregate view: one round-trip pulls permissions, pairs, and the
  // resources catalog for this store. Client-instance directory is a
  // global lookup, so it stays as a separate query.
  const view = useQuery<AdminWrapperPermissionsView>({
    queryKey: ["admin", "wrapper-permissions-view", storeId],
    queryFn: () => fetchWrapperPermissionsView(storeId!),
    enabled: !!storeId && available,
  });
  const {
    clientInstances: clientInstanceItems,
    displayNameById,
    isSuccess: clientMetadataReady,
  } = useClientInstanceDirectory();

  const resources = view.data?.resources ?? [];
  const { nameById: resourceNameById } = useResourceCatalog(resources);

  const groups = useMemo(
    () => groupPairsByResource(view.data?.pairs ?? [], resourceNameById),
    [view.data, resourceNameById],
  );
  const permissions = view.data?.permissions ?? [];
  const [filter, setFilter] = useState("");
  const [clientFilter, setClientFilter] = useState("");
  const needle = filter.trim().toLowerCase();
  const visibleGroups = groups
    .map((group) => ({
      ...group,
      pairs: group.pairs.filter((pair) =>
        [
          group.resource,
          resourceNameById.get(group.resource) ?? "",
          pair.wrapper,
        ].some((value) => value.toLowerCase().includes(needle)),
      ),
    }))
    .filter((group) => group.pairs.length > 0);
  const clientDetails = useMemo(
    () =>
      new Map(
        clientInstanceItems
          .filter((client) => client.current_store_id === storeId)
          .map((client) => [client.id, client]),
      ),
    [clientInstanceItems, storeId],
  );
  const totalPermissionCount = permissions.length;
  const writablePermissionCount = permissions.filter((p) => p.writable).length;

  const clientInstances = useMemo(
    () =>
      clientInstancesForStore(storeId ?? "", clientInstanceItems, permissions),
    [storeId, clientInstanceItems, permissions],
  );

  const visibleClients = clientInstances.filter((id) =>
    [id, displayNameById.get(id) ?? ""].some((value) =>
      value.toLowerCase().includes(clientFilter.trim().toLowerCase()),
    ),
  );

  /** Map `(client_instance, wrapper, resource)` → permission. */
  const permissionIndex = useMemo(() => {
    const map = new Map<string, WrapperPermission>();
    for (const p of permissions) {
      map.set(permissionKey(p.client_instance, p.wrapper, p.resource_id), p);
    }
    return map;
  }, [permissions]);

  const [editing, setEditing] = useState<{
    clientInstance: string;
    wrapper: string;
    resource_id: string;
    permission: WrapperPermission | null;
  } | null>(null);
  const [rawOpen, setRawOpen] = useState(false);
  const [creatingPair, setCreatingPair] = useState(false);
  const [deletingPair, setDeletingPair] =
    useState<WrapperResourcePairRecord | null>(null);
  const [transientStoreId, setTransientStoreId] = useState(storeId);
  const transientStateMatchesStore = transientStoreId === storeId;

  useEffect(() => {
    if (transientStoreId === storeId) return;
    setFilter("");
    setClientFilter("");
    setEditing(null);
    setRawOpen(false);
    setCreatingPair(false);
    setDeletingPair(null);
    setTransientStoreId(storeId);
  }, [storeId, transientStoreId]);
  usePageIntent("create", available, (value) => {
    if (value === "pair") setCreatingPair(true);
  });

  if (!storeId) {
    return (
      <div className="space-y-4">
        <h2 className="text-xl font-semibold text-(--color-fg)">
          Wrapper permissions
        </h2>
        {stores.isLoading ? (
          <p className="text-sm text-(--color-muted)">Loading…</p>
        ) : stores.error ? (
          <Alert>{describeApiError(stores.error)}</Alert>
        ) : (
          <Alert variant="warning" className="text-sm">
            No stores are configured.{" "}
            <Link
              className="underline"
              to="/stores"
              search={{ create: "store" }}
            >
              Create a store
            </Link>{" "}
            or follow{" "}
            <Link className="underline" to="/setup">
              Get a wrapper working
            </Link>
            .
          </Alert>
        )}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <header className="flex flex-wrap items-baseline justify-between gap-3">
        <div>
          <div className="flex items-center gap-3">
            <h2 className="text-xl font-semibold text-(--color-fg)">
              Wrapper permissions
            </h2>
            <span
              data-testid="writable-permissions-badge"
              className="rounded-md border border-(--color-line) bg-(--color-surface-1) px-2 py-0.5 font-mono text-xs text-(--color-fg)"
              title="Saved writable flags, before client override and admission checks"
            >
              {writablePermissionCount} saved writable of {totalPermissionCount}
            </span>
          </div>
          <p className="mt-1 text-sm text-(--color-muted)">
            Wrapper/resource pairs registered with the backend, and the saved
            permission for each runtime client instance. Click a cell to change
            its individual mode and writable flag. Client-wide overrides are
            managed in{" "}
            <Link
              className="underline"
              to="/client-instances"
              search={{ store: storeId, mappedStore: storeId }}
            >
              Client Instances
            </Link>{" "}
            without changing these rows.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <Button
            type="button"
            size="sm"
            onClick={() => setCreatingPair(true)}
            data-testid="wrappers-create-pair"
            disabled={!available}
          >
            <Plus className="mr-1.5 h-4 w-4" aria-hidden />
            Create pair
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => setRawOpen(true)}
            disabled={!available}
          >
            <FileText className="mr-1.5 h-4 w-4" aria-hidden />
            View raw policy
          </Button>
          <label className="text-sm text-(--color-muted)">
            Store:{" "}
            <Select
              data-testid="wrappers-store-select"
              value={storeId}
              onChange={(e) => setStoreId(e.target.value)}
              className="ml-1 h-auto w-auto px-2 py-1"
            >
              {(stores.data?.items ?? []).map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </Select>
          </label>
        </div>
      </header>

      <StoreAvailability store={selectedStore} />
      <div className="flex flex-wrap gap-3">
        <div className="min-w-64 flex-1 space-y-1">
          <Label htmlFor="wrapper-filter">Filter wrappers and resources</Label>
          <Input
            id="wrapper-filter"
            type="search"
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
            disabled={!available}
          />
        </div>
        <div className="min-w-56 space-y-1">
          <Label htmlFor="permission-client-filter">
            Filter client columns
          </Label>
          <Input
            id="permission-client-filter"
            type="search"
            value={clientFilter}
            onChange={(event) => setClientFilter(event.target.value)}
            disabled={!available}
          />
        </div>
      </div>
      {available && view.isSuccess ? (
        <p className="text-xs text-(--color-muted)">
          {visibleGroups.reduce(
            (count, group) => count + group.pairs.length,
            0,
          )}{" "}
          of {view.data.pairs.length} pairs; {visibleClients.length} of{" "}
          {clientInstances.length} client columns.
        </p>
      ) : null}
      {!available ? null : view.isLoading ? (
        <p className="text-sm text-(--color-muted)">
          Loading wrapper permissions…
        </p>
      ) : view.error ? (
        <Alert>{describeApiError(view.error)}</Alert>
      ) : groups.length === 0 ? (
        <p className="text-sm text-(--color-muted)">
          No wrapper/resource pairs are registered for this store yet. Use
          Create pair, or follow{" "}
          <Link className="underline" to="/setup" search={{ store: storeId }}>
            Get a wrapper working
          </Link>{" "}
          to explicitly choose manual creation or asset auto-creation and
          restart the matching frontend daemon.
        </p>
      ) : visibleGroups.length === 0 ? (
        <p className="text-sm text-(--color-muted)">
          No pairs match this metadata filter.
        </p>
      ) : (
        <ResourceGroupedPairsTable
          groups={visibleGroups}
          clientInstances={visibleClients}
          clientDetails={clientDetails}
          clientMetadataReady={clientMetadataReady}
          storeId={storeId}
          displayNameById={displayNameById}
          resourceNameById={resourceNameById}
          permissionIndex={permissionIndex}
          onCellClick={(wr, clientInstance, permission) =>
            setEditing({
              clientInstance,
              wrapper: wr.wrapper,
              resource_id: wr.resource_id,
              permission,
            })
          }
          onDeletePair={(pair) => setDeletingPair(pair)}
        />
      )}

      {transientStateMatchesStore ? (
        <>
          {editing ? (
            <EditCellDialog
              storeId={storeId}
              clientInstance={editing.clientInstance}
              clientInstanceDisplayName={
                displayNameById.get(editing.clientInstance) ??
                editing.clientInstance
              }
              wrapper={editing.wrapper}
              resourceId={editing.resource_id}
              resourceName={
                resourceNameById.get(editing.resource_id) ?? editing.resource_id
              }
              existing={editing.permission}
              accessOverride={
                clientMetadataReady
                  ? clientDetails.get(editing.clientInstance)?.access_override
                  : undefined
              }
              onClose={() => setEditing(null)}
            />
          ) : null}

          {creatingPair ? (
            <CreatePairDialog
              storeId={storeId}
              existingResources={resources}
              onClose={() => setCreatingPair(false)}
            />
          ) : null}

          {deletingPair ? (
            <DeletePairDialog
              storeId={storeId}
              pair={deletingPair}
              displayNameById={displayNameById}
              resourceName={
                resourceNameById.get(deletingPair.resource_id) ??
                deletingPair.resource_id
              }
              onClose={() => setDeletingPair(null)}
            />
          ) : null}

          <PolicyViewerDialog
            key={storeId}
            storeId={storeId}
            open={rawOpen}
            permissions={permissions}
            onClose={() => setRawOpen(false)}
          />
        </>
      ) : null}
    </div>
  );
}
