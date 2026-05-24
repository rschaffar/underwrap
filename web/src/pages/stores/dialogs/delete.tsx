import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { components } from "@/api/schema";
import {
  type AdminStoreListItem,
  deleteStore,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

type CleanupHint = components["schemas"]["AdminStoreDeleteCleanupHint"];

export function DeleteStoreDialog({
  store,
  onClose,
}: {
  store: AdminStoreListItem;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [force, setForce] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [cleanupHint, setCleanupHint] = useState<CleanupHint | null>(null);

  const mut = useMutation({
    mutationFn: (opts: { force: boolean }) => deleteStore(store.id, opts),
    onSuccess: async (res) => {
      await qc.invalidateQueries({ queryKey: ["admin", "stores"] });
      if (res.cleanup_hint) {
        // Surface the hint and require an explicit dismissal so the
        // operator notices the data dir still has secrets on disk.
        setCleanupHint(res.cleanup_hint);
      } else {
        onClose();
      }
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  const requiresForce = !store.actions.can_delete_now && store.actions.can_force_delete;
  const blockedByMapping =
    !store.actions.can_delete_now &&
    !store.actions.can_force_delete &&
    store.usage.mapped_client_instance_count > 0;
  const canDelete =
    !mut.isPending &&
    (store.actions.can_delete_now || (store.actions.can_force_delete && force));
  if (cleanupHint) {
    return (
      <Dialog open onOpenChange={(o) => !o && onClose()}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Store deleted</DialogTitle>
            <DialogDescription>
              The YAML definition for <strong>{store.name}</strong> was
              removed. The data directory still contains secrets on
              disk — follow up manually.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-2 text-xs text-(--color-muted)">
            <p>
              Data directory left in place at{" "}
              <code className="text-(--color-text)">{cleanupHint.data_dir}</code>.
              Back it up first if needed, then remove it manually:
            </p>
            <pre className="overflow-x-auto rounded-md border border-(--color-line) bg-(--color-surface-1) p-3">
              {cleanupHint.command}
            </pre>
          </div>
          <DialogFooter>
            <Button onClick={onClose}>Close</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete store: {store.name}</DialogTitle>
          <DialogDescription>
            This removes the store definition and stops the backend
            from serving it. Secret material on disk is not touched.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          {blockedByMapping ? (
            <Alert variant="warning">
              This store is still mapped to {store.usage.mapped_client_instance_count} client
              {store.usage.mapped_client_instance_count === 1 ? "" : "s"}. Remap or disable those
              instances before deleting the store.
            </Alert>
          ) : null}
          {requiresForce ? (
            <Alert variant="warning">
              This store currently contains secrets. Deleting it requires an explicit force-delete
              confirmation; the backend will remove only the store record and leave the on-disk data
              directory in place for manual follow-up.
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
          {submitError ? <Alert>{submitError}</Alert> : null}
        </div>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            variant="destructive"
            disabled={!canDelete}
            onClick={() => mut.mutate({ force: requiresForce || force })}
          >
            {mut.isPending ? "Deleting…" : requiresForce ? "Force delete" : "Delete"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
