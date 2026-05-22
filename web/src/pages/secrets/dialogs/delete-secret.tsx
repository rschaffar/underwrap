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
import { postSecretDelete } from "@/api/client";
import { describeApiError } from "@/lib/error-message";

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
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: () =>
      postSecretDelete({
        store: storeName,
        resource_id: address.resource_id,
        slot: address.slot,
        key: address.key,
      }),
    onSuccess: async () => {
      await qc.invalidateQueries({
        queryKey: ["admin", "secrets", storeName],
      });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete secret</DialogTitle>
          <DialogDescription>
            Removes{" "}
            <code>
              {address.resource_id} · {address.slot} · {address.key}
            </code>{" "}
            from <code>{storeName}</code>. This cannot be undone.
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
            disabled={mut.isPending}
            onClick={() => mut.mutate()}
          >
            {mut.isPending ? "Deleting…" : "Delete"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
