import { useState } from "react";
import { useMutation } from "@tanstack/react-query";

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
  postPairDelete,
  type WrapperResourcePairRecord,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

export function DeletePairDialog({
  storeName,
  pair,
  displayNameById,
  resourceName,
  onClose,
  onDeleted,
}: {
  storeName: string;
  pair: WrapperResourcePairRecord;
  displayNameById: Map<string, string>;
  resourceName: string;
  onClose: () => void;
  onDeleted: () => void;
}) {
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: () =>
      postPairDelete({
        store: storeName,
        wrapper: pair.wrapper,
        resource_id: pair.resource_id,
      }),
  });

  async function onDelete() {
    setSubmitError(null);
    try {
      const result = await mut.mutateAsync();
      if (result.ok) {
        onDeleted();
      }
      // result.ok === false means the backend refused; render the
      // blockers inline (handled below from `mut.data`).
    } catch (e) {
      setSubmitError(describeApiError(e));
    }
  }

  const blockers = mut.data?.blocked_by;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            Delete pair: {pair.wrapper} / {resourceName}
          </DialogTitle>
          <DialogDescription>
            Removes the wrapper/resource pair record. Any wrapper
            permissions that reference this pair must be removed first.
          </DialogDescription>
        </DialogHeader>
        {blockers ? (
          <Alert variant="warning">
            Cannot delete — {blockers.permission_count} wrapper permission
            {blockers.permission_count === 1 ? "" : "s"} still reference this pair:
            <ul className="mt-1 list-disc pl-5">
              {(blockers.sample_permissions ?? []).map((p) => (
                <li
                  key={`${p.client_instance}|${p.wrapper}|${p.resource_id}`}
                  className="font-mono"
                >
                  {displayNameById.get(p.client_instance) ?? p.client_instance}
                </li>
              ))}
            </ul>
            <p className="mt-2 text-amber-200/80">
              Clear those permissions in the table above, then try again.
            </p>
          </Alert>
        ) : null}
        {submitError ? <Alert>{submitError}</Alert> : null}
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            variant="destructive"
            onClick={() => void onDelete()}
            disabled={mut.isPending}
            data-testid="wrappers-delete-pair-confirm"
          >
            {mut.isPending ? "Deleting…" : "Delete"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
