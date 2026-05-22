import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus, FileText } from "lucide-react";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { PolicyViewerDialog } from "@/components/policy-viewer-dialog";
import {
  type AdminPairsListResponse,
  type AdminStoresListResponse,
  type AdminWrapperPermissionsListResponse,
  fetchStores,
  postPairsList,
  postWrapperPermissionsList,
  type WrapperPermission,
  type WrapperResourcePairRecord,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";
import { useStoreSelector } from "@/hooks/use-store-selector";
import { useStoreLookups } from "@/hooks/use-store-lookups";

import {
  clientInstancesForStore,
  deriveClientInstances,
  groupPairsByResource,
  sortPairs,
} from "./derivations";
import { ResourceGroupedPairsTable } from "./table";
import { CreatePairDialog } from "./dialogs/create-pair";
import { DeletePairDialog } from "./dialogs/delete-pair";
import { EditCellDialog } from "./dialogs/edit-cell";

export function WrapperPermissionsPage() {
  useEffect(() => {
    document.title = "underwrap — wrapper permissions";
  }, []);

  const qc = useQueryClient();

  const stores = useQuery<AdminStoresListResponse>({
    queryKey: ["admin", "stores"],
    queryFn: fetchStores,
  });
  const { storeName, setStoreName } = useStoreSelector(stores.data?.items);

  const permissions = useQuery<AdminWrapperPermissionsListResponse>({
    queryKey: ["admin", "wrapper-permissions", storeName],
    queryFn: () => postWrapperPermissionsList({ store: storeName! }),
    enabled: !!storeName,
  });
  const pairs = useQuery<AdminPairsListResponse>({
    queryKey: ["admin", "pairs", storeName],
    queryFn: () => postPairsList({ store: storeName! }),
    enabled: !!storeName,
  });
  const {
    resources,
    clientInstances: clientInstanceItems,
    displayNameById,
    resourceNameById,
  } = useStoreLookups(storeName);

  const pairRecords = useMemo(
    () => sortPairs(pairs.data?.pairs ?? []),
    [pairs.data],
  );
  const clientInstances = useMemo(
    () =>
      storeName
        ? clientInstancesForStore(
            storeName,
            clientInstanceItems,
            permissions.data?.permissions ?? [],
          )
        : deriveClientInstances(permissions.data?.permissions ?? []),
    [storeName, clientInstanceItems, permissions.data],
  );

  /** Map `(client_instance, wrapper, resource)` → permission. */
  const permissionIndex = useMemo(() => {
    const map = new Map<string, WrapperPermission>();
    for (const p of permissions.data?.permissions ?? []) {
      map.set(`${p.client_instance}|${p.wrapper}|${p.resource_id}`, p);
    }
    return map;
  }, [permissions.data]);

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

  function invalidatePairsAndPermissions() {
    if (!storeName) return;
    qc.invalidateQueries({ queryKey: ["admin", "pairs", storeName] });
    qc.invalidateQueries({ queryKey: ["admin", "wrapper-permissions", storeName] });
  }

  if (!storeName) {
    return (
      <div className="space-y-4">
        <h2 className="text-xl font-semibold text-(--color-fg)">
          Wrapper permissions
        </h2>
        {stores.isLoading ? (
          <p className="text-sm text-(--color-muted)">Loading…</p>
        ) : (
          <Alert variant="warning" className="text-sm">
            No stores are visible to you. Ask an admin for access to
            at least one store.
          </Alert>
        )}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <header className="flex flex-wrap items-baseline justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold text-(--color-fg)">
            Wrapper permissions
          </h2>
          <p className="mt-1 text-sm text-(--color-muted)">
            Wrapper/resource pairs registered with the backend, and the
            permission each runtime client instance holds against them.
            Click a cell to set or change the mode.
          </p>
        </div>
        <div className="flex items-center gap-3">
          <Button
            type="button"
            size="sm"
            onClick={() => setCreatingPair(true)}
            data-testid="wrappers-create-pair"
          >
            <Plus className="mr-1.5 h-4 w-4" aria-hidden />
            Create pair
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => setRawOpen(true)}
          >
            <FileText className="mr-1.5 h-4 w-4" aria-hidden />
            View raw policy
          </Button>
          <label className="text-sm text-(--color-muted)">
            Store:{" "}
            <Select
              data-testid="wrappers-store-select"
              value={storeName}
              onChange={(e) => setStoreName(e.target.value)}
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

      {permissions.isLoading || pairs.isLoading ? (
        <p className="text-sm text-(--color-muted)">
          Loading wrapper permissions…
        </p>
      ) : permissions.error || pairs.error ? (
        <p role="alert" className="text-sm text-rose-300">
          {describeApiError(permissions.error ?? pairs.error)}
        </p>
      ) : pairRecords.length === 0 ? (
        <p className="text-sm text-(--color-muted)">
          No wrapper/resource pairs are registered for this store yet.
          {" "}Use Create pair to add one, or enable auto-create on a client instance so its wrappers register on startup.
        </p>
      ) : (
        <ResourceGroupedPairsTable
          groups={groupPairsByResource(pairRecords)}
          clientInstances={clientInstances}
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

      {editing ? (
        <EditCellDialog
          storeName={storeName}
          clientInstance={editing.clientInstance}
          clientInstanceDisplayName={
            displayNameById.get(editing.clientInstance) ?? editing.clientInstance
          }
          wrapper={editing.wrapper}
          resourceId={editing.resource_id}
          resourceName={
            resourceNameById.get(editing.resource_id) ?? editing.resource_id
          }
          existing={editing.permission}
          onClose={() => setEditing(null)}
        />
      ) : null}

      {creatingPair ? (
        <CreatePairDialog
          storeName={storeName}
          existingResources={resources}
          existingPairs={pairRecords}
          onClose={() => setCreatingPair(false)}
          onCreated={() => {
            invalidatePairsAndPermissions();
            qc.invalidateQueries({ queryKey: ["admin", "resources", storeName] });
            setCreatingPair(false);
          }}
        />
      ) : null}

      {deletingPair ? (
        <DeletePairDialog
          storeName={storeName}
          pair={deletingPair}
          displayNameById={displayNameById}
          resourceName={
            resourceNameById.get(deletingPair.resource_id) ?? deletingPair.resource_id
          }
          onClose={() => setDeletingPair(null)}
          onDeleted={() => {
            invalidatePairsAndPermissions();
            setDeletingPair(null);
          }}
        />
      ) : null}

      <PolicyViewerDialog
        storeName={storeName}
        open={rawOpen}
        onClose={() => setRawOpen(false)}
      />
    </div>
  );
}
