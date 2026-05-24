import { useMutation, useQueryClient } from "@tanstack/react-query";

import { ConfirmDialog } from "@/components/confirm-dialog";
import { deleteSecret } from "@/api/client";

import type { SecretAddress } from "../model";

export function DeleteSecretDialog({
  storeId,
  address,
  onClose,
}: {
  storeId: string;
  address: SecretAddress;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const mut = useMutation({
    mutationFn: () =>
      deleteSecret(storeId, address.resource_id, address.slot, address.key),
    onSuccess: async () => {
      await qc.invalidateQueries({
        queryKey: ["admin", "secrets-view", storeId],
      });
      onClose();
    },
  });
  return (
    <ConfirmDialog
      title="Delete secret"
      description={
        <>
          Removes{" "}
          <code>
            {address.resource_id} · {address.slot} · {address.key}
          </code>{" "}
          from <code>{storeId}</code>. This cannot be undone.
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
