import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Plus } from "lucide-react";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import {
  type AdminSecretsView,
  type AdminStoresListResponse,
  fetchSecretsView,
  fetchStores,
} from "@/api/client";
import { useDocumentTitle } from "@/hooks/use-document-title";
import { describeApiError } from "@/lib/error-message";
import { useStoreSelector } from "@/hooks/use-store-selector";
import { useClientInstanceDirectory } from "@/hooks/use-client-instance-directory";
import { useResourceCatalog } from "@/hooks/use-resource-catalog";

import {
  type SecretAddress,
  type SecretResourceGroup,
  type SecretSlot,
  groupSecretEntries,
} from "./grouping";
import { ResourceBlock } from "./resource-block";
import { RevealSecretDialog } from "./dialogs/reveal";
import { WriteSecretDialog } from "./dialogs/write";
import { CreateResourceDialog } from "./dialogs/create-resource";
import { DeleteSecretDialog } from "./dialogs/delete-secret";
import { RenameResourceDialog } from "./dialogs/rename-resource";
import { DeleteResourceDialog } from "./dialogs/delete-resource";

export function SecretsPage() {
  useDocumentTitle("underwrap — secrets");

  const stores = useQuery<AdminStoresListResponse>({
    queryKey: ["admin", "stores"],
    queryFn: fetchStores,
  });
  const { storeId, setStoreId } = useStoreSelector(stores.data?.items);

  const list = useQuery<AdminSecretsView>({
    queryKey: ["admin", "secrets-view", storeId],
    queryFn: () => fetchSecretsView(storeId!),
    enabled: !!storeId,
  });
  const { displayNameById } = useClientInstanceDirectory();
  const resources = list.data?.resources ?? [];
  const { nameById: resourceNameById, recordById: resourceProvenance } =
    useResourceCatalog(resources);

  const grouped = useMemo(
    () => groupSecretEntries(list.data?.entries ?? []),
    [list.data],
  );

  const allResources = useMemo<SecretResourceGroup[]>(() => {
    const byResource = new Map<string, SecretResourceGroup>();
    for (const g of grouped) {
      byResource.set(g.resource_id, g);
    }
    for (const record of resources) {
      if (!byResource.has(record.id)) {
        byResource.set(record.id, {
          resource_id: record.id,
          env: [],
          files: [],
        });
      }
    }
    return [...byResource.values()].sort((a, b) => {
      const an = resourceNameById.get(a.resource_id) ?? a.resource_id;
      const bn = resourceNameById.get(b.resource_id) ?? b.resource_id;
      return an.localeCompare(bn);
    });
  }, [grouped, resources, resourceNameById]);

  const [revealing, setRevealing] = useState<SecretAddress | null>(null);
  const [editing, setEditing] = useState<SecretAddress | null>(null);
  const [creating, setCreating] = useState<
    { resource_id: string; slot: SecretSlot } | null
  >(null);
  const [creatingResource, setCreatingResource] = useState(false);
  const [deleting, setDeleting] = useState<SecretAddress | null>(null);
  const [deletingResource, setDeletingResource] = useState<string | null>(null);
  const [renamingResource, setRenamingResource] = useState<string | null>(null);

  if (!storeId) {
    return (
      <div className="space-y-4">
        <h2 className="text-xl font-semibold text-(--color-fg)">Secrets</h2>
        {stores.isLoading ? (
          <p className="text-sm text-(--color-muted)">Loading…</p>
        ) : (
          <Alert variant="warning" className="text-sm">
            No stores are visible to you. Ask an admin for a grant on
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
          <h2 className="text-xl font-semibold text-(--color-fg)">Secrets</h2>
          <p className="mt-1 text-sm text-(--color-muted)">
            Stored credential material. Per-resource toggles in each
            block decide whether the wrapper can write back to
            <code>files</code> or <code>libsecret</code> entries;
            admin writes through this page are always allowed.
          </p>
        </div>
        <div className="flex items-center gap-3">
          <Button
            type="button"
            size="sm"
            onClick={() => setCreatingResource(true)}
          >
            <Plus className="mr-1.5 h-4 w-4" aria-hidden />
            Create resource
          </Button>
          <label className="text-sm text-(--color-muted)">
            Store:{" "}
            <Select
              data-testid="secrets-store-select"
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

      {list.isLoading ? (
        <p className="text-sm text-(--color-muted)">Loading…</p>
      ) : list.error ? (
        <Alert>{describeApiError(list.error)}</Alert>
      ) : allResources.length === 0 ? (
        <p className="text-sm text-(--color-muted)">
          No resources are known to this store yet. Use{" "}
          <strong className="text-(--color-fg)">Create resource</strong>{" "}
          to add one, or wait for a wrapper to auto-create one on
          startup (when its client instance is opted in).
        </p>
      ) : (
        <div className="space-y-2">
          {allResources.map((group) => (
            <ResourceBlock
              key={group.resource_id}
              group={group}
              storeId={storeId}
              provenance={resourceProvenance.get(group.resource_id)}
              displayNameById={displayNameById}
              onReveal={(entry) =>
                setRevealing({
                  resource_id: group.resource_id,
                  slot: entry.slot,
                  key: entry.key,
                })
              }
              onEdit={(entry) =>
                setEditing({
                  resource_id: group.resource_id,
                  slot: entry.slot,
                  key: entry.key,
                })
              }
              onDelete={(entry) =>
                setDeleting({
                  resource_id: group.resource_id,
                  slot: entry.slot,
                  key: entry.key,
                })
              }
              onDeleteResource={() => setDeletingResource(group.resource_id)}
              onRenameResource={() => setRenamingResource(group.resource_id)}
              onAddEntry={(slot) =>
                setCreating({ resource_id: group.resource_id, slot })
              }
            />
          ))}
        </div>
      )}

      {revealing ? (
        <RevealSecretDialog
          storeId={storeId}
          address={revealing}
          onClose={() => setRevealing(null)}
        />
      ) : null}
      {creating ? (
        <WriteSecretDialog
          storeId={storeId}
          mode="create"
          address={{ ...creating, key: "" }}
          onClose={() => setCreating(null)}
        />
      ) : null}
      {creatingResource ? (
        <CreateResourceDialog
          storeId={storeId}
          existingResources={resources.map((r) => r.name)}
          onClose={() => setCreatingResource(false)}
        />
      ) : null}
      {editing ? (
        <WriteSecretDialog
          storeId={storeId}
          mode="edit"
          address={editing}
          onClose={() => setEditing(null)}
        />
      ) : null}
      {deleting ? (
        <DeleteSecretDialog
          storeId={storeId}
          address={deleting}
          onClose={() => setDeleting(null)}
        />
      ) : null}
      {deletingResource ? (
        <DeleteResourceDialog
          storeId={storeId}
          resourceId={deletingResource}
          resourceName={resourceNameById.get(deletingResource) ?? deletingResource}
          onClose={() => setDeletingResource(null)}
        />
      ) : null}
      {renamingResource ? (
        <RenameResourceDialog
          storeId={storeId}
          resourceId={renamingResource}
          currentName={
            resourceNameById.get(renamingResource) ?? renamingResource
          }
          existingNames={resources.map((r) => r.name)}
          onClose={() => setRenamingResource(null)}
        />
      ) : null}
    </div>
  );
}
