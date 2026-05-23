import { useMutation, useQueryClient } from "@tanstack/react-query";

import { ConfirmDialog } from "@/components/confirm-dialog";
import { deleteSecret } from "@/api/client";

import type { SecretAddress } from "../model";

export function DeleteSecretDialog({
  storeName,
  address,
  onClose,
}: {
  storeName: string;
  address: SecretAddress;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const mut = useMutation({
    mutationFn: () =>
      deleteSecret(storeName, address.resource_id, address.slot, address.key),
    onSuccess: async () => {
      await qc.invalidateQueries({
        queryKey: ["admin", "secrets-view", storeName],
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
          from <code>{storeName}</code>. This cannot be undone.
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
