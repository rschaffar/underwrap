import { useMutation } from "@tanstack/react-query";

import { ConfirmDialog } from "@/components/confirm-dialog";
import {
  type AdminClientInstanceListItem,
  deleteClientInstance,
} from "@/api/client";

export function DeleteClientInstanceDialog({
  instance,
  onClose,
  onSuccess,
}: {
  instance: AdminClientInstanceListItem;
  onClose: () => void;
  onSuccess: () => Promise<void>;
}) {
  const mut = useMutation({
    mutationFn: () => deleteClientInstance(instance.id),
    onSuccess: async () => {
      await onSuccess();
    },
  });

  return (
    <ConfirmDialog
      title={`Delete client instance: ${instance.display_name}`}
      description="This removes the client instance record permanently. The client's registered certificate will no longer be accepted for runtime requests."
      destructive
      confirmLabel="Delete"
      pendingLabel="Deleting…"
      onConfirm={() => mut.mutateAsync()}
      onClose={onClose}
    />
  );
}
