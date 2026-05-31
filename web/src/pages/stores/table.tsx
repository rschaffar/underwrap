import { Button } from "@/components/ui/button";
import { StatusChip } from "@/components/ui/status-chip";
import type { AdminStoresListResponse } from "@/api/client";

function storeDeleteTitle(
  item: AdminStoresListResponse["items"][number],
): string | undefined {
  if (item.actions.can_delete_now) return undefined;
  if (item.actions.can_force_delete) {
    return "Store contains secrets — a force delete confirmation is required.";
  }
  if (item.usage.mapped_client_instance_count > 0) {
    const count = item.usage.mapped_client_instance_count;
    return `Store is still mapped to ${count} client instance${count === 1 ? "" : "s"}. Remap them first.`;
  }
  return "Store cannot currently be deleted.";
}

export interface StoresTableProps {
  items: AdminStoresListResponse["items"];
  onEdit: (item: AdminStoresListResponse["items"][number]) => void;
  onDelete: (item: AdminStoresListResponse["items"][number]) => void;
  onUnlock: (item: AdminStoresListResponse["items"][number]) => void;
  onLock: (item: AdminStoresListResponse["items"][number]) => void;
  onRotateKey: (item: AdminStoresListResponse["items"][number]) => void;
}

export function StoresTable({
  items,
  onEdit,
  onDelete,
  onUnlock,
  onLock,
  onRotateKey,
}: StoresTableProps) {
  if (items.length === 0) {
    return (
      <p className="text-sm text-(--color-muted)">No stores configured.</p>
    );
  }
  return (
    <div className="overflow-x-auto rounded-xl border border-(--color-line)">
      <table className="w-full text-left text-sm">
        <thead className="bg-(--color-surface-0)/60 text-xs uppercase tracking-wide text-(--color-muted)">
          <tr>
            <th className="px-4 py-2">Name</th>
            <th className="px-4 py-2">Backend</th>
            <th className="px-4 py-2">Status</th>
            <th className="px-4 py-2">Usage</th>
            <th className="px-4 py-2">Description</th>
            <th className="px-4 py-2 text-right">Actions</th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => {
            const canDelete =
              item.actions.can_delete_now || item.actions.can_force_delete;
            const canUnlock = item.actions.can_unlock;
            const canLock = item.actions.can_lock;
            const canRotate = item.actions.can_rotate_key;
            return (
              <tr
                key={item.id}
                className="border-t border-(--color-line) align-middle"
              >
                <td className="px-4 py-3 font-medium text-(--color-fg)">
                  <div>{item.name}</div>
                  <div className="text-xs text-(--color-muted)">{item.id}</div>
                </td>
                <td className="px-4 py-3 text-(--color-muted)">
                  {item.backend}
                </td>
                <td className="px-4 py-3">
                  {item.locked ? (
                    <StatusChip
                      tone="amber"
                      data-testid={`store-status-${item.id}`}
                    >
                      locked
                    </StatusChip>
                  ) : item.backend === "encrypted" ? (
                    <StatusChip
                      tone="emerald"
                      data-testid={`store-status-${item.id}`}
                    >
                      unlocked
                    </StatusChip>
                  ) : (
                    <span
                      data-testid={`store-status-${item.id}`}
                      className="text-xs text-(--color-muted)"
                    >
                      —
                    </span>
                  )}
                </td>
                <td className="px-4 py-3 text-(--color-muted)">
                  <div>
                    {item.usage.mapped_client_instance_count} client
                    {item.usage.mapped_client_instance_count === 1 ? "" : "s"}
                  </div>
                  <div className="text-xs">
                    {item.locked
                      ? "contents unknown while locked"
                      : item.usage.has_secrets
                        ? "contains secrets"
                        : "empty"}
                  </div>
                </td>
                <td className="px-4 py-3 text-(--color-muted)">
                  {item.description ?? "—"}
                </td>
                <td className="px-4 py-3 text-right">
                  <div className="inline-flex gap-2">
                    {canUnlock ? (
                      <Button
                        variant="default"
                        size="sm"
                        data-testid={`store-unlock-${item.id}`}
                        onClick={() => onUnlock(item)}
                      >
                        Unlock
                      </Button>
                    ) : null}
                    {canLock ? (
                      <Button
                        variant="ghost"
                        size="sm"
                        data-testid={`store-lock-${item.id}`}
                        onClick={() => onLock(item)}
                      >
                        Lock
                      </Button>
                    ) : null}
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => onEdit(item)}
                    >
                      Edit
                    </Button>
                    {canRotate ? (
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => onRotateKey(item)}
                      >
                        Rotate key
                      </Button>
                    ) : null}
                    <Button
                      variant="destructive"
                      size="sm"
                      disabled={!canDelete}
                      title={storeDeleteTitle(item)}
                      onClick={() => onDelete(item)}
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
