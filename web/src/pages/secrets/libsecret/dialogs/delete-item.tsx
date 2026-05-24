import { ConfirmDialog } from "@/components/confirm-dialog";
import { useConfirmMutation } from "@/hooks/use-confirm-mutation";
import { deleteLibsecretItem } from "@/api/client";

import { libsecretTreeKey } from "../helpers";

export function DeleteItemDialog({
  storeId,
  resourceId,
  collectionId,
  itemId,
  onClose,
}: {
  storeId: string;
  resourceId: string;
  collectionId: string;
  itemId: string;
  onClose: () => void;
}) {
  const { confirm, pending, submitError } = useConfirmMutation({
    mutationFn: () =>
      deleteLibsecretItem(storeId, resourceId, collectionId, itemId),
    invalidate: [libsecretTreeKey(storeId, resourceId)],
    onSuccess: onClose,
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
      onConfirm={() => confirm()}
      onClose={onClose}
      pending={pending}
      submitError={submitError}
    />
  );
}
