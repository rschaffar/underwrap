import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { type AdminStoresListResponse, fetchStores } from "@/api/client";
import { useDocumentTitle } from "@/hooks/use-document-title";
import { usePageIntent } from "@/hooks/use-page-intent";
import { describeApiError } from "@/lib/error-message";

import { StoresTable } from "./table";
import { CreateStoreDialog } from "./dialogs/create";
import { EditStoreDialog } from "./dialogs/edit";
import { DeleteStoreDialog } from "./dialogs/delete";
import { UnlockStoreDialog } from "./dialogs/unlock";
import { LockStoreDialog } from "./dialogs/lock";
import { RotateStoreKeyDialog } from "./dialogs/rotate-key";
import { MigrateStoreDialog } from "./dialogs/migrate";

export function StoresPage() {
  useDocumentTitle("underwrap — stores");

  const { data, error, isLoading } = useQuery<AdminStoresListResponse>({
    queryKey: ["admin", "stores"],
    queryFn: fetchStores,
  });

  const [createOpen, setCreateOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<
    AdminStoresListResponse["items"][number] | null
  >(null);
  const [unlockingId, setUnlockingId] = useState<string | null>(null);
  const [lockingId, setLockingId] = useState<string | null>(null);
  const [rotatingId, setRotatingId] = useState<string | null>(null);
  const [migratingId, setMigratingId] = useState<string | null>(null);

  usePageIntent("create", !isLoading && !error, (value) => {
    if (value === "store") setCreateOpen(true);
  });
  usePageIntent("unlock", !isLoading && !error, (id) => {
    if (
      data?.items.some((store) => store.id === id && store.actions.can_unlock)
    )
      setUnlockingId(id);
  });

  const items = data?.items ?? [];
  const editing = items.find((item) => item.id === editingId) ?? null;
  const unlocking = items.find((item) => item.id === unlockingId) ?? null;
  const locking = items.find((item) => item.id === lockingId) ?? null;
  const rotating = items.find((item) => item.id === rotatingId) ?? null;
  const migrating = items.find((item) => item.id === migratingId) ?? null;

  return (
    <div className="space-y-6">
      <header className="flex flex-wrap items-baseline justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold text-(--color-fg)">Stores</h2>
          <p className="mt-1 text-sm text-(--color-muted)">
            Stores are backend-managed records. Create, edit, and delete them
            here.
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
          items={items}
          onEdit={setEditingId}
          onDelete={(storeId) =>
            setDeleting(items.find((item) => item.id === storeId) ?? null)
          }
          onUnlock={setUnlockingId}
          onLock={setLockingId}
          onRotateKey={setRotatingId}
          onMigrate={setMigratingId}
        />
      )}

      {createOpen ? (
        <CreateStoreDialog onClose={() => setCreateOpen(false)} />
      ) : null}
      {editing ? (
        <EditStoreDialog store={editing} onClose={() => setEditingId(null)} />
      ) : null}
      {deleting ? (
        <DeleteStoreDialog store={deleting} onClose={() => setDeleting(null)} />
      ) : null}
      {unlocking ? (
        <UnlockStoreDialog
          store={unlocking}
          onClose={() => setUnlockingId(null)}
        />
      ) : null}
      {locking ? (
        <LockStoreDialog store={locking} onClose={() => setLockingId(null)} />
      ) : null}
      {rotatingId ? (
        <RotateStoreKeyDialog
          storeId={rotatingId}
          currentStore={rotating}
          onClose={() => setRotatingId(null)}
        />
      ) : null}
      {migratingId ? (
        <MigrateStoreDialog
          storeId={migratingId}
          currentStore={migrating}
          onClose={() => setMigratingId(null)}
        />
      ) : null}
    </div>
  );
}
