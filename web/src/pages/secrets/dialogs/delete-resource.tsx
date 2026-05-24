import { useMutation, useQueryClient } from "@tanstack/react-query";

import { Alert } from "@/components/ui/alert";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { deleteResource } from "@/api/client";

type DeleteResourceResult = Awaited<ReturnType<typeof deleteResource>>;
type Blockers = NonNullable<DeleteResourceResult["blocked_by"]>;

function BlockersAlert({ blockers }: { blockers: Blockers | null | undefined }) {
  if (!blockers) return null;
  return (
    <Alert
      variant="warning"
      className="space-y-2"
      data-testid="resource-delete-blockers"
    >
      <p>Cannot delete — the following still reference this resource:</p>
      <ul className="list-disc pl-5">
        {blockers.secret_count > 0 ? (
          <li>
            <strong>{blockers.secret_count}</strong> secret
            {blockers.secret_count === 1 ? "" : "s"}
            {(blockers.sample_secret_ids ?? []).length > 0 ? (
              <>
                {" "}
                (e.g.{" "}
                <code className="font-mono">
                  {(blockers.sample_secret_ids ?? []).join(", ")}
                </code>
                )
              </>
            ) : null}
          </li>
        ) : null}
        {blockers.pair_count > 0 ? (
          <li>
            <strong>{blockers.pair_count}</strong> wrapper/resource
            pair{blockers.pair_count === 1 ? "" : "s"}
            {(blockers.sample_pairs ?? []).length > 0 ? (
              <>
                {" "}
                (e.g.{" "}
                <code className="font-mono">
                  {(blockers.sample_pairs ?? [])
                    .map((p) => `${p.wrapper}/${p.resource}`)
                    .join(", ")}
                </code>
                )
              </>
            ) : null}
          </li>
        ) : null}
        {blockers.permission_count > 0 ? (
          <li>
            <strong>{blockers.permission_count}</strong> wrapper permission
            {blockers.permission_count === 1 ? "" : "s"}
          </li>
        ) : null}
      </ul>
      <p className="text-amber-200/80">
        Clear those dependents first, then try again.
      </p>
    </Alert>
  );
}

export function DeleteResourceDialog({
  storeId,
  resourceId,
  resourceName,
  onClose,
}: {
  storeId: string;
  resourceId: string;
  resourceName: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const mut = useMutation({
    mutationFn: () => deleteResource(storeId, resourceId),
  });
  async function onConfirm() {
    const result = await mut.mutateAsync();
    if (result.ok) {
      // The secrets and wrapper-permissions pages both surface
      // resources via their aggregate views, so invalidating both
      // view keys keeps either page consistent on next visit.
      await Promise.all([
        qc.invalidateQueries({
          queryKey: ["admin", "secrets-view", storeId],
        }),
        qc.invalidateQueries({
          queryKey: ["admin", "wrapper-permissions-view", storeId],
        }),
      ]);
      onClose();
    }
    // result.ok === false → backend reported blockers; rendered below.
  }
  return (
    <ConfirmDialog
      title={`Delete resource: ${resourceName}`}
      description="Removes the resource record. The backend refuses if any secrets, wrapper/resource pairs, or grants still reference this resource."
      body={<BlockersAlert blockers={mut.data?.blocked_by} />}
      destructive
      confirmLabel="Delete"
      pendingLabel="Deleting…"
      confirmTestId="resource-delete-confirm"
      onConfirm={onConfirm}
      onClose={onClose}
    />
  );
}
