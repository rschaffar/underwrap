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
  type AdminClientInstanceListItem,
  deleteClientInstance,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

export function DeleteClientInstanceDialog({
  instance,
  onClose,
  onSuccess,
}: {
  instance: AdminClientInstanceListItem;
  onClose: () => void;
  onSuccess: () => Promise<void>;
}) {
  const [submitError, setSubmitError] = useState<string | null>(null);

  const mut = useMutation({
    mutationFn: () => deleteClientInstance(instance.id),
    onSuccess: async () => {
      setSubmitError(null);
      await onSuccess();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  const canDelete = !mut.isPending;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete client instance: {instance.display_name}</DialogTitle>
          <DialogDescription>
            This removes the client instance record permanently. The
            client's registered certificate will no longer be accepted
            for runtime requests.
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
            disabled={!canDelete}
            onClick={() => mut.mutate()}
          >
            {mut.isPending ? "Deleting…" : "Delete"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
