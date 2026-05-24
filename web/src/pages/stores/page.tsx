import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { type AdminStoreListItem, type AdminStoresListResponse, fetchStores } from "@/api/client";
import { useDocumentTitle } from "@/hooks/use-document-title";
import { describeApiError } from "@/lib/error-message";

import { StoresTable } from "./table";
import { CreateStoreDialog } from "./dialogs/create";
import { EditStoreDialog } from "./dialogs/edit";
import { DeleteStoreDialog } from "./dialogs/delete";
import { UnlockStoreDialog } from "./dialogs/unlock";
import { LockStoreDialog } from "./dialogs/lock";
import { RotateStoreKeyDialog } from "./dialogs/rotate-key";

export function StoresPage() {
  useDocumentTitle("underwrap — stores");

  const { data, error, isLoading } = useQuery<AdminStoresListResponse>({
    queryKey: ["admin", "stores"],
    queryFn: fetchStores,
  });

  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState<AdminStoreListItem | null>(null);
  const [deleting, setDeleting] = useState<AdminStoreListItem | null>(
    null,
  );
  const [unlocking, setUnlocking] = useState<AdminStoreListItem | null>(
    null,
  );
  const [locking, setLocking] = useState<AdminStoreListItem | null>(
    null,
  );
  const [rotating, setRotating] = useState<AdminStoreListItem | null>(
    null,
  );

  return (
    <div className="space-y-6">
      <header className="flex flex-wrap items-baseline justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold text-(--color-fg)">Stores</h2>
          <p className="mt-1 text-sm text-(--color-muted)">
            Stores are backend-managed records. Create, edit, and delete
            them here.
          </p>
        </div>
        <Button onClick={() => setCreateOpen(true)}>Create store</Button>
      </header>

      {isLoading ? (
        <p className="text-sm text-(--color-muted)">Loading…</p>
      ) : error ? (
        <Alert>{describeApiError(error)}</Alert>
      ) : (
        <StoresTable
          items={data?.items ?? []}
          onEdit={(item) => setEditing(item)}
          onDelete={(item) => setDeleting(item)}
          onUnlock={(item) => setUnlocking(item)}
          onLock={(item) => setLocking(item)}
          onRotateKey={(item) => setRotating(item)}
        />
      )}

      {createOpen ? (
        <CreateStoreDialog
          open={createOpen}
          onClose={() => setCreateOpen(false)}
        />
      ) : null}
      {editing ? (
        <EditStoreDialog
          store={editing}
          onClose={() => setEditing(null)}
        />
      ) : null}
      {deleting ? (
        <DeleteStoreDialog
          store={deleting}
          onClose={() => setDeleting(null)}
        />
      ) : null}
      {unlocking ? (
        <UnlockStoreDialog
          store={unlocking}
          onClose={() => setUnlocking(null)}
        />
      ) : null}
      {locking ? (
        <LockStoreDialog
          store={locking}
          onClose={() => setLocking(null)}
        />
      ) : null}
      {rotating ? (
        <RotateStoreKeyDialog
          store={rotating}
          onClose={() => setRotating(null)}
        />
      ) : null}
    </div>
  );
}
