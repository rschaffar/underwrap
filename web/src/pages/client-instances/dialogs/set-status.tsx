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
  putClientInstanceUpdate,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

export function SetStatusDialog({
  instance,
  nextStatus,
  onClose,
  onSuccess,
}: {
  instance: AdminClientInstanceListItem;
  nextStatus: "active" | "disabled";
  onClose: () => void;
  onSuccess: () => Promise<void>;
}) {
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: (status: "active" | "disabled") => putClientInstanceUpdate(instance.id, { status }),
    onSuccess: async () => {
      setSubmitError(null);
      await onSuccess();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {nextStatus === "disabled" ? "Disable" : "Re-enable"}: {instance.display_name}
          </DialogTitle>
          <DialogDescription>
            {nextStatus === "disabled"
              ? "Disabling this instance denies new runtime requests immediately. Existing list visibility is retained so operators can still manage it."
              : "Re-enabling this instance allows new runtime requests again immediately."}
          </DialogDescription>
        </DialogHeader>
        {submitError ? <Alert>{submitError}</Alert> : null}
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            variant={nextStatus === "disabled" ? "destructive" : "default"}
            disabled={mut.isPending}
            onClick={() => mut.mutate(nextStatus)}
          >
            {mut.isPending
              ? nextStatus === "disabled"
                ? "Disabling…"
                : "Updating…"
              : nextStatus === "disabled"
                ? "Disable now"
                : "Re-enable now"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
