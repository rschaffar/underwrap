import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Copy, Check } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  postPolicyGet,
  type AdminPolicyGetResponse,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

interface PolicyViewerDialogProps {
  storeName: string;
  open: boolean;
  onClose: () => void;
}

/**
 * Read-only viewer for a store's `PolicyDocument`. The Matrix page
 * shows a derived `(wrapper, host, user)` grid that hides the
 * underlying document shape; this dialog renders the raw JSON the
 * backend stores, plus the global `writes_enabled` kill-switch as a
 * prominent badge.
 *
 * Editing intentionally stays in the Matrix view — see ticket 301
 * for the scope boundary.
 */
export function PolicyViewerDialog({
  storeName,
  open,
  onClose,
}: PolicyViewerDialogProps) {
  const policy = useQuery<AdminPolicyGetResponse>({
    queryKey: ["admin", "policy", storeName],
    queryFn: () => postPolicyGet({ store: storeName }),
    enabled: open,
    // The viewer is read-only and operators reopen it to confirm
    // recent edits. Cache freshness matters more than save round
    // trips; refetch every time the dialog mounts.
    staleTime: 0,
    refetchOnMount: "always",
  });

  const formatted = useMemo(() => {
    if (!policy.data) return "";
    return JSON.stringify(policy.data.document, null, 2);
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

  const filesWritable = policy.data?.document.files_writable ?? {};
  const libsecretWritable = policy.data?.document.libsecret_writable ?? {};
  const filesOnCount = Object.values(filesWritable).filter(Boolean).length;
  const libsecretOnCount = Object.values(libsecretWritable).filter(Boolean).length;

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle>Raw policy: {storeName}</DialogTitle>
          <DialogDescription>
            The persisted <code>PolicyDocument</code> for this store.
            Read-only — use the Matrix view to change grants.
          </DialogDescription>
        </DialogHeader>

        {policy.isLoading ? (
          <p className="text-sm text-(--color-muted)">Loading policy…</p>
        ) : policy.error ? (
          <p
            role="alert"
            className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
          >
            {describeApiError(policy.error)}
          </p>
        ) : (
          <div className="space-y-3">
            <div className="flex items-center gap-3 text-xs">
              <span className="text-(--color-muted)">
                Wrapper writes:
              </span>
              <span
                data-testid="writable-files-badge"
                className="rounded-md border border-(--color-line) bg-(--color-surface-1) px-2 py-0.5 font-mono text-(--color-fg)"
              >
                files: {filesOnCount} on
              </span>
              <span
                data-testid="writable-libsecret-badge"
                className="rounded-md border border-(--color-line) bg-(--color-surface-1) px-2 py-0.5 font-mono text-(--color-fg)"
              >
                libsecret: {libsecretOnCount} on
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
      </DialogContent>
    </Dialog>
  );
}
