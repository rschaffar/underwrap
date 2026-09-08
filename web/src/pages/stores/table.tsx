import { Button } from "@/components/ui/button";
import { StatusChip, type ChipTone } from "@/components/ui/status-chip";
import type { AdminStoresListResponse } from "@/api/client";

type StoreItem = AdminStoresListResponse["items"][number];

function storeDeleteTitle(item: StoreItem): string | undefined {
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

function lifecycleTone(item: StoreItem): ChipTone {
  switch (item.lifecycle_state) {
    case "ready":
    case "unlocked":
      return "emerald";
    case "locked":
    case "busy":
      return "amber";
    case "recovery-required":
      return "neutral";
  }
}

function migrationLabel(item: StoreItem): string {
  if (item.lifecycle_state === "ready") return "Migrate to encrypted";
  if (item.lifecycle_state === "unlocked") return "Migrate to plain";
  return "Migrate";
}

function migrationTitle(item: StoreItem): string | undefined {
  if (item.lifecycle_state === "locked") {
    return "Unlock this encrypted store before migration.";
  }
  if (item.lifecycle_state === "busy") {
    return "Migration is disabled while a lifecycle operation is busy.";
  }
  if (item.lifecycle_state === "recovery-required") {
    return "Trusted-host recovery is required before migration.";
  }
  if (!item.actions.can_migrate) return "Store cannot currently be migrated.";
  return undefined;
}

export interface StoresTableProps {
  items: AdminStoresListResponse["items"];
  onEdit: (storeId: string) => void;
  onDelete: (storeId: string) => void;
  onUnlock: (storeId: string) => void;
  onLock: (storeId: string) => void;
  onRotateKey: (storeId: string) => void;
  onMigrate: (storeId: string) => void;
}

export function StoresTable({
  items,
  onEdit,
  onDelete,
  onUnlock,
  onLock,
  onRotateKey,
  onMigrate,
}: StoresTableProps) {
  if (items.length === 0) {
    return <p className="text-sm text-(--color-muted)">No stores configured.</p>;
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
            const migrationGuidance = item.lifecycle_state === "locked";
            const migrationDisabled =
              !item.actions.can_migrate && !migrationGuidance;
            const contentsKnown =
              item.lifecycle_state === "ready" ||
              item.lifecycle_state === "unlocked";
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
                  <StatusChip
                    tone={lifecycleTone(item)}
                    data-testid={`store-status-${item.id}`}
                  >
                    {item.lifecycle_state}
                  </StatusChip>
                </td>
                <td className="px-4 py-3 text-(--color-muted)">
                  <div>
                    {item.usage.mapped_client_instance_count} client
                    {item.usage.mapped_client_instance_count === 1 ? "" : "s"}
                  </div>
                  <div className="text-xs">
                    {!contentsKnown
                      ? `contents unknown while ${item.lifecycle_state}`
                      : item.usage.has_secrets
                        ? "contains secrets"
                        : "empty"}
                  </div>
                </td>
                <td className="px-4 py-3 text-(--color-muted)">
                  {item.description ?? "—"}
                </td>
                <td className="px-4 py-3 text-right">
                  <div className="inline-flex flex-wrap justify-end gap-2">
                    {item.actions.can_unlock ? (
                      <Button
                        variant="default"
                        size="sm"
                        data-testid={`store-unlock-${item.id}`}
                        onClick={() => onUnlock(item.id)}
                      >
                        Unlock
                      </Button>
                    ) : null}
                    {item.actions.can_lock ? (
                      <Button
                        variant="ghost"
                        size="sm"
                        data-testid={`store-lock-${item.id}`}
                        onClick={() => onLock(item.id)}
                      >
                        Lock
                      </Button>
                    ) : null}
                    <Button
                      variant="ghost"
                      size="sm"
                      disabled={!item.actions.can_update}
                      onClick={() => onEdit(item.id)}
                    >
                      Edit
                    </Button>
                    {item.actions.can_rotate_key ? (
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => onRotateKey(item.id)}
                      >
                        Rotate key
                      </Button>
                    ) : null}
                    <Button
                      variant="ghost"
                      size="sm"
                      disabled={migrationDisabled}
                      title={migrationTitle(item)}
                      onClick={() => onMigrate(item.id)}
                    >
                      {migrationLabel(item)}
                    </Button>
                    <Button
                      variant="destructive"
                      size="sm"
                      disabled={!canDelete}
                      title={storeDeleteTitle(item)}
                      onClick={() => onDelete(item.id)}
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
