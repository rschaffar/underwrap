import { useMutation, useQueryClient } from "@tanstack/react-query";

import { ConfirmDialog } from "@/components/confirm-dialog";
import { deleteLibsecretAlias } from "@/api/client";

import { libsecretTreeKey } from "../internal";

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
  const qc = useQueryClient();
  const mut = useMutation({
    mutationFn: () => deleteLibsecretAlias(storeId, resourceId, name),
    onSuccess: async () => {
      await qc.invalidateQueries({
        queryKey: libsecretTreeKey(storeId, resourceId),
      });
      onClose();
    },
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
      onConfirm={() => mut.mutateAsync()}
      onClose={onClose}
    />
  );
}
