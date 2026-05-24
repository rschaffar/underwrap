import { useMutation, useQueryClient } from "@tanstack/react-query";

import { ConfirmDialog } from "@/components/confirm-dialog";
import { deleteLibsecretItem } from "@/api/client";

import { libsecretTreeKey } from "../internal";

export function DeleteItemDialog({
  storeName,
  resourceId,
  collectionId,
  itemId,
  onClose,
}: {
  storeName: string;
  resourceId: string;
  collectionId: string;
  itemId: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const mut = useMutation({
    mutationFn: () =>
      deleteLibsecretItem(storeName, resourceId, collectionId, itemId),
    onSuccess: async () => {
      await qc.invalidateQueries({
        queryKey: libsecretTreeKey(storeName, resourceId),
      });
      onClose();
    },
  });
  return (
    <ConfirmDialog
      title="Delete item"
      description={
        <>
          Removes <code>{collectionId} · {itemId}</code> from resource{" "}
          <code>{resourceId}</code>. This cannot be undone.
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
