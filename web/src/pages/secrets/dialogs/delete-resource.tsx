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
import { postResourceDelete } from "@/api/client";
import { describeApiError } from "@/lib/error-message";

export function DeleteResourceDialog({
  storeName,
  resourceId,
  resourceName,
  onClose,
}: {
  storeName: string;
  resourceId: string;
  resourceName: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: () =>
      postResourceDelete({ store: storeName, id: resourceId }),
  });
  async function onDelete() {
    setSubmitError(null);
    try {
      const result = await mut.mutateAsync();
      if (result.ok) {
        await Promise.all([
          qc.invalidateQueries({ queryKey: ["admin", "resources", storeName] }),
          qc.invalidateQueries({ queryKey: ["admin", "secrets", storeName] }),
          qc.invalidateQueries({ queryKey: ["admin", "pairs", storeName] }),
        ]);
        onClose();
      }
      // result.ok === false → backend reported blockers; rendered below.
    } catch (e) {
      setSubmitError(describeApiError(e));
    }
  }
  const blockers = mut.data?.blocked_by;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete resource: {resourceName}</DialogTitle>
          <DialogDescription>
            Removes the resource record. The backend refuses if any
            secrets, wrapper/resource pairs, or grants still reference
            this resource.
          </DialogDescription>
        </DialogHeader>
        {blockers ? (
          <Alert
            variant="warning"
            className="space-y-2"
            data-testid="resource-delete-blockers"
          >
            <p>Cannot delete — the following still reference this resource:</p>
            <ul className="list-disc pl-5">
              {blockers.secret_count > 0 ? (
                <li>
                  <strong>{blockers.secret_count}</strong> secret
                  {blockers.secret_count === 1 ? "" : "s"}
                  {(blockers.sample_secret_ids ?? []).length > 0 ? (
                    <>
                      {" "}
                      (e.g.{" "}
                      <code className="font-mono">
                        {(blockers.sample_secret_ids ?? []).join(", ")}
                      </code>
                      )
                    </>
                  ) : null}
                </li>
              ) : null}
              {blockers.pair_count > 0 ? (
                <li>
                  <strong>{blockers.pair_count}</strong> wrapper/resource
                  pair{blockers.pair_count === 1 ? "" : "s"}
                  {(blockers.sample_pairs ?? []).length > 0 ? (
                    <>
                      {" "}
                      (e.g.{" "}
                      <code className="font-mono">
                        {(blockers.sample_pairs ?? [])
                          .map((p) => `${p.wrapper}/${p.resource}`)
                          .join(", ")}
                      </code>
                      )
                    </>
                  ) : null}
                </li>
              ) : null}
              {blockers.permission_count > 0 ? (
                <li>
                  <strong>{blockers.permission_count}</strong> wrapper permission
                  {blockers.permission_count === 1 ? "" : "s"}
                </li>
              ) : null}
            </ul>
            <p className="text-amber-200/80">
              Clear those dependents first, then try again.
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
            disabled={mut.isPending}
            onClick={() => void onDelete()}
            data-testid="resource-delete-confirm"
          >
            {mut.isPending ? "Deleting…" : "Delete"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
