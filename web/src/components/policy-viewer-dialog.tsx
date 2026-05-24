import { useMemo, useState } from "react";
import { Copy, Check } from "lucide-react";

import { AdminDialog } from "@/components/admin-dialog";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";
import type { WrapperPermission } from "@/api/client";

interface PolicyViewerDialogProps {
  storeId: string;
  open: boolean;
  permissions: WrapperPermission[];
  onClose: () => void;
}

/**
 * Read-only viewer for a store's `PolicyDocument`. The Wrapper
 * permissions page shows a derived
 * `(wrapper, resource, client_instance)` grid that hides the
 * underlying document shape; this dialog renders the raw JSON the
 * backend stores, plus a quick count of writable permissions.
 *
 * Permissions arrive as a prop from the parent's wrapper-permissions
 * view query, so opening the dialog doesn't fire a second
 * round-trip for the same data.
 */
export function PolicyViewerDialog({
  storeId,
  open,
  permissions,
  onClose,
}: PolicyViewerDialogProps) {
  // Render a `PolicyDocument`-shaped blob even though the wire format
  // is just a permissions array; operators paste this into docs and
  // tickets, so the on-screen shape stays stable across the
  // policy/get → wrapper-permissions/list switch.
  const formatted = useMemo(
    () => JSON.stringify({ permissions }, null, 2),
    [permissions],
  );

  const [copied, setCopied] = useState(false);
  async function onCopy() {
    try {
      await navigator.clipboard.writeText(formatted);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Older browsers / non-secure contexts: leave the badge as-is.
      // The pre block is selectable so the operator can still copy
      // manually.
    }
  }

  const writablePermissionCount = permissions.filter((p) => p.writable).length;

  return (
    <AdminDialog
      open={open}
      onClose={onClose}
      title={`Raw policy: ${storeId}`}
      description={
        <>
          The persisted <code>PolicyDocument</code> for this store.
          Read-only — use the Wrapper permissions view to change
          permissions.
        </>
      }
    >
      <div className="space-y-3">
        <div className="flex items-center gap-3 text-xs">
          <span className="text-(--color-muted)">Writable permissions:</span>
          <span
            data-testid="writable-permissions-badge"
            className="rounded-md border border-(--color-line) bg-(--color-surface-1) px-2 py-0.5 font-mono text-(--color-fg)"
          >
            {writablePermissionCount} of {permissions.length}
          </span>
        </div>
        <pre
          data-testid="policy-json"
          className="max-h-[60vh] overflow-auto rounded-md border border-(--color-line) bg-(--color-surface-0) p-3 text-xs leading-relaxed text-(--color-fg)"
        >
          {formatted}
        </pre>
      </div>

      <DialogFooter>
        <Button type="button" variant="ghost" onClick={onCopy}>
          {copied ? (
            <>
              <Check className="mr-1.5 h-4 w-4" aria-hidden /> Copied
            </>
          ) : (
            <>
              <Copy className="mr-1.5 h-4 w-4" aria-hidden /> Copy JSON
            </>
          )}
        </Button>
        <Button type="button" onClick={onClose}>
          Close
        </Button>
      </DialogFooter>
    </AdminDialog>
  );
}
