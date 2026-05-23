import { useMutation, useQueryClient } from "@tanstack/react-query";

import { ConfirmDialog } from "@/components/confirm-dialog";
import {
  type AdminClientInstanceListItem,
  deleteClientInstance,
} from "@/api/client";

export function DeleteClientInstanceDialog({
  instance,
  onClose,
}: {
  instance: AdminClientInstanceListItem;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const mut = useMutation({
    mutationFn: () => deleteClientInstance(instance.id),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "client-instances"] });
      onClose();
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
