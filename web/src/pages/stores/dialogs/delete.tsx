import { useState } from "react";

import { Alert } from "@/components/ui/alert";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { useConfirmMutation } from "@/hooks/use-confirm-mutation";
import type { components } from "@/api/schema";
import { type AdminStoreListItem, deleteStore } from "@/api/client";

import { CleanupHintDialog } from "./cleanup-hint";

type CleanupHint = components["schemas"]["AdminStoreDeleteCleanupHint"];

export function DeleteStoreDialog({
  store,
  onClose,
}: {
  store: AdminStoreListItem;
  onClose: () => void;
}) {
  const [force, setForce] = useState(false);
  const [cleanupHint, setCleanupHint] = useState<CleanupHint | null>(null);

  const requiresForce = !store.actions.can_delete_now && store.actions.can_force_delete;
  const blockedByMapping =
    !store.actions.can_delete_now &&
    !store.actions.can_force_delete &&
    store.usage.mapped_client_instance_count > 0;

  const { confirm, pending, submitError } = useConfirmMutation({
    mutationFn: (opts: { force: boolean }) => deleteStore(store.id, opts),
    invalidate: [["admin", "stores"]],
  });

  async function onDelete() {
    const result = await confirm({ force: requiresForce || force });
    if (result.cleanup_hint) {
      setCleanupHint(result.cleanup_hint);
    } else {
      onClose();
    }
  }

  if (cleanupHint) {
    return (
      <CleanupHintDialog
        storeName={store.name}
        hint={cleanupHint}
        onClose={onClose}
      />
    );
  }

  return (
    <ConfirmDialog
      title={`Delete store: ${store.name}`}
      description="This removes the store definition and stops the backend from serving it. Secret material on disk is not touched."
      body={
        <div className="space-y-3">
          {blockedByMapping ? (
            <Alert variant="warning">
              This store is still mapped to{" "}
              {store.usage.mapped_client_instance_count} client
              {store.usage.mapped_client_instance_count === 1 ? "" : "s"}. Remap
              or disable those instances before deleting the store.
            </Alert>
          ) : null}
          {requiresForce ? (
            <Alert variant="warning">
              This store currently contains secrets. Deleting it requires an
              explicit force-delete confirmation; the backend will remove only
              the store record and leave the on-disk data directory in place
              for manual follow-up.
            </Alert>
          ) : null}
          {store.actions.can_force_delete ? (
            <label className="flex items-center gap-2 text-xs text-(--color-muted)">
              <input
                type="checkbox"
                checked={force}
                onChange={(e) => setForce(e.target.checked)}
              />
              Force delete even if the store contains secrets
            </label>
          ) : null}
        </div>
      }
      destructive
      disabled={
        !store.actions.can_delete_now &&
        !(store.actions.can_force_delete && force)
      }
      confirmLabel={requiresForce ? "Force delete" : "Delete"}
      pendingLabel="Deleting…"
      onConfirm={onDelete}
      onClose={onClose}
      pending={pending}
      submitError={submitError}
    />
  );
}
