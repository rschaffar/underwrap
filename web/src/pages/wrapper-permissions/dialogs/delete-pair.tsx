import { useMutation, useQueryClient } from "@tanstack/react-query";

import { Alert } from "@/components/ui/alert";
import { ConfirmDialog } from "@/components/confirm-dialog";
import {
  deletePair,
  type WrapperResourcePairRecord,
} from "@/api/client";

type DeletePairResult = Awaited<ReturnType<typeof deletePair>>;
type Blockers = NonNullable<DeletePairResult["blocked_by"]>;

function BlockersAlert({
  blockers,
  displayNameById,
}: {
  blockers: Blockers | null | undefined;
  displayNameById: Map<string, string>;
}) {
  if (!blockers) return null;
  return (
    <Alert variant="warning">
      Cannot delete — {blockers.permission_count} wrapper permission
      {blockers.permission_count === 1 ? "" : "s"} still reference this pair:
      <ul className="mt-1 list-disc pl-5">
        {(blockers.sample_permissions ?? []).map((p) => (
          <li
            key={`${p.client_instance}|${p.wrapper}|${p.resource_id}`}
            className="font-mono"
          >
            {displayNameById.get(p.client_instance) ?? p.client_instance}
          </li>
        ))}
      </ul>
      <p className="mt-2 text-amber-200/80">
        Clear those permissions in the table above, then try again.
      </p>
    </Alert>
  );
}

export function DeletePairDialog({
  storeName,
  pair,
  displayNameById,
  resourceName,
  onClose,
}: {
  storeName: string;
  pair: WrapperResourcePairRecord;
  displayNameById: Map<string, string>;
  resourceName: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const mut = useMutation({
    mutationFn: () => deletePair(storeName, pair.wrapper, pair.resource_id),
  });

  async function onConfirm() {
    const result = await mut.mutateAsync();
    if (result.ok) {
      await qc.invalidateQueries({
        queryKey: ["admin", "wrapper-permissions-view", storeName],
      });
      onClose();
    }
    // result.ok === false means the backend refused; render the
    // blockers inline (handled below from `mut.data`).
  }

  return (
    <ConfirmDialog
      title={`Delete pair: ${pair.wrapper} / ${resourceName}`}
      description="Removes the wrapper/resource pair record. Any wrapper permissions that reference this pair must be removed first."
      body={
        <BlockersAlert
          blockers={mut.data?.blocked_by}
          displayNameById={displayNameById}
        />
      }
      destructive
      confirmLabel="Delete"
      pendingLabel="Deleting…"
      onConfirm={onConfirm}
      onClose={onClose}
    />
  );
}
