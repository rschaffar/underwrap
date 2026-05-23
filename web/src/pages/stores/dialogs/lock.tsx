import { useMutation, useQueryClient } from "@tanstack/react-query";

import { ConfirmDialog } from "@/components/confirm-dialog";
import {
  type AdminStoreListItem,
  postStoreLock,
} from "@/api/client";

/**
 * Confirmation dialog for locking an encrypted store. Drops the
 * backend's in-memory master key so subsequent wrapper requests fail
 * with `store_locked` until an admin unlocks again.
 */
export function LockStoreDialog({
  store,
  onClose,
}: {
  store: AdminStoreListItem;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const lock = useMutation({
    mutationFn: () => postStoreLock(store.id),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "stores"] });
      onClose();
    },
  });

  return (
    <ConfirmDialog
      title={`Lock store: ${store.name}`}
      description={
        <>
          Locking drops the backend's in-memory master key for this
          store. Wrappers will receive
          <code className="mx-1 rounded bg-(--color-surface-1) px-1 py-0.5 text-xs">store_locked</code>
          until you unlock again by re-pasting the master key.
        </>
      }
      destructive
      confirmLabel="Lock"
      pendingLabel="Locking…"
      onConfirm={() => lock.mutateAsync()}
      onClose={onClose}
    />
  );
}
