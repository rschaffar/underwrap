import { Button } from "@/components/ui/button";
import type {
  AdminUserDetailResponse,
  AdminUsersListResponse,
} from "@/api/client";
import { formatEpochDate } from "@/lib/epoch";

export interface UsersTableProps {
  users: AdminUsersListResponse["users"];
  meUsername: string | null;
  onEdit: (u: AdminUserDetailResponse) => void;
  onReset: (u: AdminUserDetailResponse) => void;
  onDelete: (u: AdminUserDetailResponse) => void;
}

export function UsersTable({
  users,
  meUsername,
  onEdit,
  onReset,
  onDelete,
}: UsersTableProps) {
  if (users.length === 0) {
    return (
      <p className="text-sm text-(--color-muted)">No users configured.</p>
    );
  }
  return (
    <div className="overflow-x-auto rounded-xl border border-(--color-line)">
      <table className="w-full text-left text-sm">
        <thead className="bg-(--color-surface-0)/60 text-xs uppercase tracking-wide text-(--color-muted)">
          <tr>
            <th className="px-4 py-2">Username</th>
            <th className="px-4 py-2">Created</th>
            <th className="px-4 py-2">Password changed</th>
            <th className="px-4 py-2 text-right">Actions</th>
          </tr>
        </thead>
        <tbody>
          {users.map((u) => {
            const isMe = meUsername === u.username;
            return (
              <tr
                key={u.id}
                className="border-t border-(--color-line) align-middle"
              >
                <td className="px-4 py-3 font-medium text-(--color-fg)">
                  {u.username}
                  {isMe ? (
                    <span className="ml-2 text-xs text-(--color-muted)">
                      (you)
                    </span>
                  ) : null}
                </td>
                <td className="px-4 py-3 text-(--color-muted)">
                  {formatEpochDate(u.created_at)}
                </td>
                <td className="px-4 py-3 text-(--color-muted)">
                  {formatEpochDate(u.password_changed_at)}
                </td>
                <td className="px-4 py-3 text-right">
                  <div className="inline-flex gap-2">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => onEdit(u)}
                    >
                      Edit
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => onReset(u)}
                    >
                      Reset password
                    </Button>
                    <Button
                      variant="destructive"
                      size="sm"
                      onClick={() => onDelete(u)}
                    >
                      Delete
                    </Button>
                  </div>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
