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
import {
  type AdminStoreDetailResponse,
  postStoreLock,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

/**
 * Confirmation dialog for locking an encrypted store. Drops the
 * backend's in-memory master key so subsequent wrapper requests fail
 * with `store_locked` until an admin unlocks again.
 */
export function LockStoreDialog({
  store,
  onClose,
}: {
  store: AdminStoreDetailResponse;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const lock = useMutation({
    mutationFn: () => postStoreLock(store.id),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "stores"] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent data-testid="store-lock-dialog">
        <DialogHeader>
          <DialogTitle>Lock store: {store.name}</DialogTitle>
          <DialogDescription>
            Locking drops the backend's in-memory master key for this
            store. Wrappers will receive
            <code className="mx-1 rounded bg-(--color-surface-1) px-1 py-0.5 text-xs">store_locked</code>
            until you unlock again by re-pasting the master key.
          </DialogDescription>
        </DialogHeader>
        {submitError ? <Alert>{submitError}</Alert> : null}
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            variant="destructive"
            data-testid="store-lock-submit"
            disabled={lock.isPending}
            onClick={() => {
              setSubmitError(null);
              lock.mutate();
            }}
          >
            {lock.isPending ? "Locking…" : "Lock"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
