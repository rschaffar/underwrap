import { useMutation } from "@tanstack/react-query";

import { ConfirmDialog } from "@/components/confirm-dialog";
import {
  type AdminClientInstanceListItem,
  putClientInstanceUpdate,
} from "@/api/client";

export function SetStatusDialog({
  instance,
  nextStatus,
  onClose,
  onSuccess,
}: {
  instance: AdminClientInstanceListItem;
  nextStatus: "active" | "disabled";
  onClose: () => void;
  onSuccess: () => Promise<void>;
}) {
  const mut = useMutation({
    mutationFn: (status: "active" | "disabled") =>
      putClientInstanceUpdate(instance.id, { status }),
    onSuccess: async () => {
      await onSuccess();
    },
  });

  const disabling = nextStatus === "disabled";

  return (
    <ConfirmDialog
      title={`${disabling ? "Disable" : "Re-enable"}: ${instance.display_name}`}
      description={
        disabling
          ? "Disabling this instance denies new runtime requests immediately. Existing list visibility is retained so operators can still manage it."
          : "Re-enabling this instance allows new runtime requests again immediately."
      }
      destructive={disabling}
      confirmLabel={disabling ? "Disable now" : "Re-enable now"}
      pendingLabel={disabling ? "Disabling…" : "Updating…"}
      onConfirm={() => mut.mutateAsync(nextStatus)}
      onClose={onClose}
    />
  );
}
