import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { Alert } from "@/components/ui/alert";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { deleteResource } from "@/api/client";
import { describeApiError } from "@/lib/error-message";
import { invalidateAll } from "@/lib/query";

type DeleteResourceResult = Awaited<ReturnType<typeof deleteResource>>;
type Blockers = NonNullable<DeleteResourceResult["blocked_by"]>;
type ContentReference = NonNullable<Blockers["sample_content"]>[number];

function describeContentReference(reference: ContentReference): string {
  switch (reference.kind) {
    case "env":
      return `env: ${reference.name}`;
    case "file":
      return `file: ${reference.path}`;
    case "libsecret_alias":
      return `libsecret alias: ${reference.name}`;
    case "libsecret_collection":
      return `libsecret collection: ${reference.collection_id}`;
    case "libsecret_item":
      return `libsecret item: ${reference.collection_id}/${reference.item_id}`;
  }
}

function BlockersAlert({
  blockers,
}: {
  blockers: Blockers | null | undefined;
}) {
  if (!blockers) return null;
  return (
    <Alert
      variant="warning"
      className="space-y-2"
      data-testid="resource-delete-blockers"
    >
      <p>Cannot delete — the following still reference this resource:</p>
      <ul className="list-disc pl-5">
        {blockers.content_entry_count > 0 ? (
          <li>
            <strong>{blockers.content_entry_count}</strong> stored resource
            content {blockers.content_entry_count === 1 ? "entry" : "entries"}
            {(blockers.sample_content ?? []).length > 0 ? (
              <ul className="list-disc pl-5">
                {(blockers.sample_content ?? []).map((reference, index) => (
                  <li key={`${reference.kind}-${index}`}>
                    <code className="font-mono">
                      {describeContentReference(reference)}
                    </code>
                  </li>
                ))}
              </ul>
            ) : null}
          </li>
        ) : null}
        {blockers.pair_count > 0 ? (
          <li>
            <strong>{blockers.pair_count}</strong> wrapper/resource pair
            {blockers.pair_count === 1 ? "" : "s"}
            {(blockers.sample_pairs ?? []).length > 0 ? (
              <ul className="list-disc pl-5">
                {(blockers.sample_pairs ?? []).map((pair, index) => (
                  <li key={`${pair.wrapper}-${pair.resource}-${index}`}>
                    <code className="font-mono">
                      {pair.wrapper}/{pair.resource}
                    </code>
                  </li>
                ))}
              </ul>
            ) : null}
          </li>
        ) : null}
        {blockers.permission_count > 0 ? (
          <li>
            <strong>{blockers.permission_count}</strong> permission row
            {blockers.permission_count === 1 ? "" : "s"}
            {(blockers.sample_permissions ?? []).length > 0 ? (
              <ul className="list-disc pl-5">
                {(blockers.sample_permissions ?? []).map(
                  (permission, index) => (
                    <li
                      key={`${permission.client_instance}-${permission.wrapper}-${permission.resource_id}-${index}`}
                    >
                      <code className="font-mono">
                        {permission.client_instance}/{permission.wrapper}/
                        {permission.resource_id}
                      </code>
                    </li>
                  ),
                )}
              </ul>
            ) : null}
          </li>
        ) : null}
      </ul>
      <p className="text-(--color-warning)">
        Clear those dependents first, then try again.
      </p>
    </Alert>
  );
}

// This dialog can't use `useConfirmMutation` directly because it needs
// `mut.data?.blocked_by` for the "soft refusal" path where the backend
// returns 200 ok=false with a blockers list. We thread pending +
// submitError through the same ConfirmDialog props the hook would.
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
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: () => deleteResource(storeId, resourceId),
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  async function onConfirm() {
    setSubmitError(null);
    let result: DeleteResourceResult;
    try {
      result = await mut.mutateAsync();
    } catch {
      return;
    }
    if (result.ok) {
      // The secrets and wrapper-permissions pages both surface
      // resources via their aggregate views, so invalidating both
      // view keys keeps either page consistent on next visit.
      await invalidateAll(qc, [
        ["admin", "secrets-view", storeId],
        ["admin", "wrapper-permissions-view", storeId],
      ]);
      onClose();
    }
    // result.ok === false → backend reported blockers; rendered below.
  }
  return (
    <ConfirmDialog
      title={`Delete resource: ${resourceName}`}
      description="Removes the resource record and its active empty content blob. The backend refuses if stored resource content, wrapper/resource pairs, or permission rows still reference this resource."
      body={<BlockersAlert blockers={mut.data?.blocked_by} />}
      destructive
      confirmLabel="Delete"
      pendingLabel="Deleting…"
      confirmTestId="resource-delete-confirm"
      onConfirm={onConfirm}
      onClose={onClose}
      pending={mut.isPending}
      submitError={submitError}
    />
  );
}
