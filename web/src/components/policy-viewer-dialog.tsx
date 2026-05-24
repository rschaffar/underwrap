import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Copy, Check } from "lucide-react";

import { AdminDialog } from "@/components/admin-dialog";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";
import { fetchWrapperPermissions } from "@/api/client";
import { describeApiError } from "@/lib/error-message";

interface PolicyViewerDialogProps {
  storeId: string;
  open: boolean;
  onClose: () => void;
}

/**
 * Read-only viewer for a store's `PolicyDocument`. The Wrapper
 * permissions page shows a derived
 * `(wrapper, resource, client_instance)` grid that hides the
 * underlying document shape; this dialog renders the raw JSON the
 * backend stores, plus a quick count of writable permissions.
 *
 * Editing intentionally stays in the Wrapper permissions view.
 */
export function PolicyViewerDialog({
  storeId,
  open,
  onClose,
}: PolicyViewerDialogProps) {
  const policy = useQuery({
    queryKey: ["admin", "wrapper-permissions", storeId],
    queryFn: () => fetchWrapperPermissions(storeId),
    enabled: open,
  });

  // Render a `PolicyDocument`-shaped blob even though the wire format
  // is just a permissions array; operators paste this into docs and
  // tickets, so the on-screen shape stays stable across the
  // policy/get → wrapper-permissions/list switch.
  const formatted = useMemo(() => {
    if (!policy.data) return "";
    return JSON.stringify(
      { permissions: policy.data.permissions },
      null,
      2,
    );
  }, [policy.data]);

  const [copied, setCopied] = useState(false);
  async function onCopy() {
    if (!formatted) return;
    try {
      await navigator.clipboard.writeText(formatted);
      setCopied(true);
      // Reset the icon after a short delay so the button stays useful
      // for a follow-up paste without the user having to close the
      // dialog.
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Older browsers / non-secure contexts: leave the badge as-is.
      // The pre block is selectable so the operator can still copy
      // manually.
    }
  }

  const permissions = policy.data?.permissions ?? [];
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
      {policy.isLoading ? (
          <p className="text-sm text-(--color-muted)">Loading policy…</p>
        ) : policy.error ? (
          <Alert>{describeApiError(policy.error)}</Alert>
        ) : (
          <div className="space-y-3">
            <div className="flex items-center gap-3 text-xs">
              <span className="text-(--color-muted)">
                Writable permissions:
              </span>
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
        )}

        <DialogFooter>
          <Button
            type="button"
            variant="ghost"
            onClick={onCopy}
            disabled={!formatted}
          >
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
