import { ConfirmDialog } from "@/components/confirm-dialog";
import { useConfirmMutation } from "@/hooks/use-confirm-mutation";
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
  const { confirm, pending, submitError } = useConfirmMutation({
    mutationFn: () =>
      deleteLibsecretCollection(storeId, resourceId, collectionId),
    invalidate: [libsecretTreeKey(storeId, resourceId)],
    onSuccess: onClose,
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
      onConfirm={() => confirm()}
      onClose={onClose}
      pending={pending}
      submitError={submitError}
    />
  );
}
