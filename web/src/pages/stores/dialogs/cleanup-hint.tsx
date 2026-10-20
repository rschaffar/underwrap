import { AdminDialog } from "@/components/admin-dialog";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";
import type { components } from "@/api/schema";

type CleanupHint = components["schemas"]["AdminStoreDeleteCleanupHint"];

/**
 * Follow-up dialog opened after a store force-delete returns a
 * cleanup hint. The store record is already gone — this is purely a
 * notice to the operator that secret material is still on disk.
 */
export function CleanupHintDialog({
  storeName,
  hint,
  onClose,
}: {
  storeName: string;
  hint: CleanupHint;
  onClose: () => void;
}) {
  return (
    <AdminDialog
      open
      onClose={onClose}
      title="Store deleted"
      description={
        <>
          The store registry record for <strong>{storeName}</strong> was removed.
          The data directory was left on disk and may contain secrets — follow up
          manually.
        </>
      }
    >
      <div className="space-y-2 text-xs text-(--color-muted)">
        <p>
          Data directory left in place at{" "}
          <code className="text-(--color-text)">{hint.data_dir}</code>. Back it
          up first if needed, verify the selected path on the backend host, then
          remove it manually using POSIX shell syntax:
        </p>
        <pre className="overflow-x-auto rounded-md border border-(--color-line) bg-(--color-surface-1) p-3">
          {hint.command}
        </pre>
        <p>
          Retained generations and external backups are not removed by store
          deletion or this command. Handle them separately after reviewing
          retention and key custody. Removal is not secure erasure.
        </p>
      </div>
      <DialogFooter>
        <Button onClick={onClose}>Close</Button>
      </DialogFooter>
    </AdminDialog>
  );
}
