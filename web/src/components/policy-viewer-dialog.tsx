import { useEffect, useMemo, useRef, useState } from "react";
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
 * backend stores.
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
  // Wrap the permissions array in `{ permissions }` so the displayed
  // JSON matches the `PolicyDocument` shape operators recognise when
  // pasting it into docs and tickets.
  const formatted = useMemo(
    () => JSON.stringify({ permissions }, null, 2),
    [permissions],
  );

  const [copied, setCopied] = useState(false);
  const mounted = useRef(false);
  const resetTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (resetTimer.current !== null) clearTimeout(resetTimer.current);
    };
  }, []);

  async function onCopy() {
    try {
      await navigator.clipboard.writeText(formatted);
    } catch {
      // The pre block remains selectable without clipboard access.
      return;
    }
    if (!mounted.current) return;
    if (resetTimer.current !== null) clearTimeout(resetTimer.current);
    setCopied(true);
    resetTimer.current = setTimeout(() => {
      resetTimer.current = null;
      setCopied(false);
    }, 1500);
  }

  return (
    <AdminDialog
      open={open}
      onClose={onClose}
      title={`Raw policy: ${storeId}`}
      description={
        <>
          The persisted <code>PolicyDocument</code> for this store. Read-only —
          use the Wrapper permissions view to change permissions.
        </>
      }
    >
      <pre
        data-testid="policy-json"
        className="max-h-[60vh] overflow-auto rounded-md border border-(--color-line) bg-(--color-surface-0) p-3 text-xs leading-relaxed text-(--color-fg)"
      >
        {formatted}
      </pre>

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
