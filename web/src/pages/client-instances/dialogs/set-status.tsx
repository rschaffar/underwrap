import { useMutation, useQueryClient } from "@tanstack/react-query";

import { ConfirmDialog } from "@/components/confirm-dialog";
import {
  type AdminClientInstanceListItem,
  putClientInstanceUpdate,
} from "@/api/client";

export function SetStatusDialog({
  instance,
  nextStatus,
  onClose,
}: {
  instance: AdminClientInstanceListItem;
  nextStatus: "active" | "disabled";
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const mut = useMutation({
    mutationFn: (status: "active" | "disabled") =>
      putClientInstanceUpdate(instance.id, { status }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "client-instances"] });
      onClose();
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
