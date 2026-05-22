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
  type AdminUserDetailResponse,
  deleteUser,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

export function DeleteUserDialog({
  user,
  isLastUser,
  onClose,
}: {
  user: AdminUserDetailResponse;
  isLastUser: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: () => deleteUser(user.id),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "users"] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  const canDelete = !mut.isPending && !isLastUser;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete user: {user.username}</DialogTitle>
          <DialogDescription>
            This removes the account. Any session it holds is revoked
            server-side.
          </DialogDescription>
        </DialogHeader>
        {isLastUser ? (
          <Alert variant="warning">
            This is the last remaining user. The backend refuses to
            delete it; create another user first.
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
