import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  type AdminUserListItem,
  type AdminUsersListResponse,
  fetchUsers,
  type WhoamiResponse,
} from "@/api/client";
import { useDocumentTitle } from "@/hooks/use-document-title";
import { describeApiError } from "@/lib/error-message";

import { UsersTable } from "./table";
import { CreateUserDialog } from "./dialogs/create";
import { EditUserDialog } from "./dialogs/edit";
import { ResetPasswordDialog } from "./dialogs/reset-password";
import { DeleteUserDialog } from "./dialogs/delete";

interface UsersPageProps {
  /** Whoami payload from the router context. Used to mark the current
   *  user with "(you)" and to disable self-delete when alone. */
  principal: WhoamiResponse | null;
}

export function UsersPage({ principal }: UsersPageProps) {
  useDocumentTitle("underwrap — users");

  const meUsername = principal?.username ?? null;

  const { data, error, isLoading } = useQuery<AdminUsersListResponse>({
    queryKey: ["admin", "users"],
    queryFn: fetchUsers,
  });

  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState<AdminUserListItem | null>(null);
  const [resetting, setResetting] = useState<AdminUserListItem | null>(
    null,
  );
  const [deleting, setDeleting] = useState<AdminUserListItem | null>(
    null,
  );

  const users = data?.users ?? [];

  return (
    <div className="space-y-6">
      <header className="flex flex-wrap items-baseline justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold text-(--color-fg)">Users</h2>
          <p className="mt-1 text-sm text-(--color-muted)">
            Accounts that can sign in to this admin UI. Every user has
            full access — there are no roles or per-store restrictions.
          </p>
        </div>
        <Button onClick={() => setCreateOpen(true)}>Create user</Button>
      </header>

      {isLoading ? (
        <p className="text-sm text-(--color-muted)">Loading…</p>
      ) : error ? (
        <Alert>{describeApiError(error)}</Alert>
      ) : (
        <UsersTable
          users={users}
          meUsername={meUsername}
          onEdit={(u) => setEditing(u)}
          onReset={(u) => setResetting(u)}
          onDelete={(u) => setDeleting(u)}
        />
      )}

      {createOpen ? (
        <CreateUserDialog
          open={createOpen}
          onClose={() => setCreateOpen(false)}
        />
      ) : null}
      {editing ? (
        <EditUserDialog
          user={editing}
          onClose={() => setEditing(null)}
        />
      ) : null}
      {resetting ? (
        <ResetPasswordDialog
          user={resetting}
          onClose={() => setResetting(null)}
        />
      ) : null}
      {deleting ? (
        <DeleteUserDialog
          user={deleting}
          isLastUser={users.length <= 1}
          onClose={() => setDeleting(null)}
        />
      ) : null}
    </div>
  );
}
