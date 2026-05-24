import { ConfirmDialog } from "@/components/confirm-dialog";
import { useConfirmMutation } from "@/hooks/use-confirm-mutation";
import { api } from "@/api/client";

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
  const { confirm, pending, submitError } = useConfirmMutation({
    mutationFn: () =>
      api.DELETE("/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}", { params: { path: { id: storeId, rid: address.resource_id, slot: address.slot, key: address.key } } }),
    invalidate: [["admin", "secrets-view", storeId]],
    onSuccess: onClose,
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
      onConfirm={() => confirm()}
      onClose={onClose}
      pending={pending}
      submitError={submitError}
    />
  );
}
