import { useMutation, useQueryClient } from "@tanstack/react-query";

import { ConfirmDialog } from "@/components/confirm-dialog";
import { deleteLibsecretCollection } from "@/api/client";

import { libsecretTreeKey } from "../internal";

export function DeleteCollectionDialog({
  storeId,
  resourceId,
  collectionId,
  onClose,
}: {
  storeId: string;
  resourceId: string;
  collectionId: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const mut = useMutation({
    mutationFn: () =>
      deleteLibsecretCollection(storeId, resourceId, collectionId),
    onSuccess: async () => {
      await qc.invalidateQueries({
        queryKey: libsecretTreeKey(storeId, resourceId),
      });
      onClose();
    },
  });
  return (
    <ConfirmDialog
      title="Delete collection"
      description={
        <>
          Removes collection <code>{collectionId}</code> and all its
          items. Any aliases pointing at this collection are scrubbed
          atomically. This cannot be undone.
        </>
      }
      destructive
      confirmLabel="Delete"
      pendingLabel="Deleting…"
      onConfirm={() => mut.mutateAsync()}
      onClose={onClose}
    />
  );
}
