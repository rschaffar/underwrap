import { ConfirmDialog } from "@/components/confirm-dialog";
import { useConfirmMutation } from "@/hooks/use-confirm-mutation";
import { deleteLibsecretAlias } from "@/api/client";

import { libsecretTreeKey } from "../helpers";

export function DeleteAliasDialog({
  storeId,
  resourceId,
  name,
  onClose,
}: {
  storeId: string;
  resourceId: string;
  name: string;
  onClose: () => void;
}) {
  const { confirm, pending, submitError } = useConfirmMutation({
    mutationFn: () => deleteLibsecretAlias(storeId, resourceId, name),
    invalidate: [libsecretTreeKey(storeId, resourceId)],
    onSuccess: onClose,
  });
  return (
    <ConfirmDialog
      title="Delete alias"
      description={
        <>
          Removes alias <code>{name}</code> from resource{" "}
          <code>{resourceId}</code>. Collections it pointed at are
          untouched. This cannot be undone.
        </>
      }
      destructive
      confirmLabel="Delete"
      pendingLabel="Deleting…"
      onConfirm={confirm}
      onClose={onClose}
      pending={pending}
      submitError={submitError}
    />
  );
}
