import { useMutation, useQueryClient } from "@tanstack/react-query";

import { Alert } from "@/components/ui/alert";
import { ConfirmDialog } from "@/components/confirm-dialog";
import {
  type AdminUserListItem,
  deleteUser,
} from "@/api/client";

export function DeleteUserDialog({
  user,
  isLastUser,
  onClose,
}: {
  user: AdminUserListItem;
  isLastUser: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const mut = useMutation({
    mutationFn: () => deleteUser(user.id),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "users"] });
      onClose();
    },
  });
  return (
    <ConfirmDialog
      title={`Delete user: ${user.username}`}
      description="This removes the account. Any session it holds is revoked server-side."
      body={
        isLastUser ? (
          <Alert variant="warning">
            This is the last remaining user. The backend refuses to
            delete it; create another user first.
          </Alert>
        ) : null
      }
      destructive
      disabled={isLastUser}
      confirmLabel="Delete"
      pendingLabel="Deleting…"
      onConfirm={() => mut.mutateAsync()}
      onClose={onClose}
    />
  );
}
