import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Plus, FileText } from "lucide-react";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { PolicyViewerDialog } from "@/components/policy-viewer-dialog";
import {
  type AdminStoresListResponse,
  type AdminWrapperPermissionsView,
  fetchStores,
  fetchWrapperPermissionsView,
  type WrapperPermission,
  type WrapperResourcePairRecord,
} from "@/api/client";
import { useDocumentTitle } from "@/hooks/use-document-title";
import { describeApiError } from "@/lib/error-message";
import { useStoreSelector } from "@/hooks/use-store-selector";
import { useClientInstanceDirectory } from "@/hooks/use-store-lookups";

import {
  clientInstancesForStore,
  groupPairsByResource,
  sortPairs,
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
  const { storeName, setStoreName } = useStoreSelector(stores.data?.items);

  // Aggregate view: one round-trip pulls permissions, pairs, and the
  // resources catalog for this store. Client-instance directory is a
  // global lookup, so it stays as a separate query.
  const view = useQuery<AdminWrapperPermissionsView>({
    queryKey: ["admin", "wrapper-permissions-view", storeName],
    queryFn: () => fetchWrapperPermissionsView(storeName!),
    enabled: !!storeName,
  });
  const { clientInstances: clientInstanceItems, displayNameById } =
    useClientInstanceDirectory();

  const resources = view.data?.resources ?? [];
  const resourceNameById = useMemo(() => {
    const map = new Map<string, string>();
    for (const r of resources) {
      map.set(r.id, r.name);
    }
    return map;
  }, [resources]);

  const pairRecords = useMemo(
    () => sortPairs(view.data?.pairs ?? []),
    [view.data],
  );
  const clientInstances = useMemo(
    () =>
      clientInstancesForStore(
        storeName ?? "",
        clientInstanceItems,
        view.data?.permissions ?? [],
      ),
    [storeName, clientInstanceItems, view.data],
  );

  /** Map `(client_instance, wrapper, resource)` → permission. */
  const permissionIndex = useMemo(() => {
    const map = new Map<string, WrapperPermission>();
    for (const p of view.data?.permissions ?? []) {
      map.set(`${p.client_instance}|${p.wrapper}|${p.resource_id}`, p);
    }
    return map;
  }, [view.data]);

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

      {view.isLoading ? (
        <p className="text-sm text-(--color-muted)">
          Loading wrapper permissions…
        </p>
      ) : view.error ? (
        <Alert>{describeApiError(view.error)}</Alert>
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
