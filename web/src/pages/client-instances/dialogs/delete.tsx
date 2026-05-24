import { ConfirmDialog } from "@/components/confirm-dialog";
import { useConfirmMutation } from "@/hooks/use-confirm-mutation";
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
  const { confirm, pending, submitError } = useConfirmMutation({
    mutationFn: () => deleteClientInstance(instance.id),
    invalidate: [["admin", "client-instances"]],
    onSuccess: onClose,
  });
  return (
    <ConfirmDialog
      title={`Delete client instance: ${instance.display_name}`}
      description="This removes the client instance record permanently. The client's registered certificate will no longer be accepted for runtime requests."
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
