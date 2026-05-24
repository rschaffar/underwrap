import { Alert } from "@/components/ui/alert";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { useConfirmMutation } from "@/hooks/use-confirm-mutation";
import { type AdminUserListItem, deleteUser } from "@/api/client";

export function DeleteUserDialog({
  user,
  isLastUser,
  onClose,
}: {
  user: AdminUserListItem;
  isLastUser: boolean;
  onClose: () => void;
}) {
  const { confirm, pending, submitError } = useConfirmMutation({
    mutationFn: () => deleteUser(user.id),
    invalidate: [["admin", "users"]],
    onSuccess: onClose,
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
      onConfirm={() => confirm()}
      onClose={onClose}
      pending={pending}
      submitError={submitError}
    />
  );
}
