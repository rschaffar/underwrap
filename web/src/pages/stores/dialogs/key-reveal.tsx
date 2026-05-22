import { type ReactNode, useState } from "react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";

/**
 * Shown after a successful encrypted-store create or rotate-key call.
 * The key value lives only in this component's state — it is dropped
 * the moment the operator confirms the acknowledgement checkbox and
 * closes the dialog. Never written to localStorage, sessionStorage,
 * cookies, or the URL.
 */
export function KeyRevealDialog({
  title,
  description,
  keyHex,
  onClose,
}: {
  title: string;
  description: ReactNode;
  keyHex: string;
  onClose: () => void;
}) {
  const [ack, setAck] = useState(false);
  const [copyStatus, setCopyStatus] = useState<"idle" | "copied" | "failed">(
    "idle",
  );

  async function onCopy() {
    try {
      if (typeof navigator !== "undefined" && navigator.clipboard) {
        await navigator.clipboard.writeText(keyHex);
        setCopyStatus("copied");
      } else {
        setCopyStatus("failed");
      }
    } catch {
      setCopyStatus("failed");
    }
  }

  return (
    <Dialog
      open
      onOpenChange={(o) => {
        // Only allow closing via the explicit confirm button after the
        // ack box is ticked. Ignore escape / outside clicks while the
        // key is visible so a stray click can't bury it.
        if (!o && ack) onClose();
      }}
    >
      <DialogContent
        data-testid="key-reveal-dialog"
        onEscapeKeyDown={(e) => {
          if (!ack) e.preventDefault();
        }}
        onPointerDownOutside={(e) => {
          if (!ack) e.preventDefault();
        }}
        onInteractOutside={(e) => {
          if (!ack) e.preventDefault();
        }}
      >
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <Label htmlFor="key-reveal-value" className="text-xs">
            Master key (hex)
          </Label>
          <pre
            id="key-reveal-value"
            data-testid="key-reveal-value"
            className="overflow-x-auto rounded-md border border-(--color-line) bg-(--color-surface-1) p-3 font-mono text-xs text-(--color-fg)"
          >
            {keyHex}
          </pre>
          <div className="flex items-center gap-2">
            <Button type="button" size="sm" variant="ghost" onClick={onCopy}>
              Copy to clipboard
            </Button>
            {copyStatus === "copied" ? (
              <span className="text-xs text-emerald-300">Copied.</span>
            ) : copyStatus === "failed" ? (
              <span className="text-xs text-rose-300">
                Could not access clipboard — select the value above and
                copy by hand.
              </span>
            ) : null}
          </div>
          <label className="flex items-start gap-2 text-xs text-(--color-muted)">
            <input
              type="checkbox"
              data-testid="key-reveal-ack"
              checked={ack}
              onChange={(e) => setAck(e.target.checked)}
            />
            <span>I have saved this key securely.</span>
          </label>
        </div>
        <DialogFooter>
          <Button
            type="button"
            data-testid="key-reveal-confirm"
            disabled={!ack}
            onClick={onClose}
          >
            Done
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
