import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { Button } from "@/components/ui/button";
import {
  type AdminUserDetailResponse,
  type AdminUsersListResponse,
  fetchUsers,
  type WhoamiResponse,
} from "@/api/client";
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
  useEffect(() => {
    document.title = "underwrap — users";
  }, []);

  const meUsername = principal?.username ?? null;

  const { data, error, isLoading } = useQuery<AdminUsersListResponse>({
    queryKey: ["admin", "users"],
    queryFn: fetchUsers,
  });

  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState<AdminUserDetailResponse | null>(null);
  const [resetting, setResetting] = useState<AdminUserDetailResponse | null>(
    null,
  );
  const [deleting, setDeleting] = useState<AdminUserDetailResponse | null>(
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
        <p role="alert" className="text-sm text-rose-300">
          {describeApiError(error)}
        </p>
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
