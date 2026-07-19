import { type ReactNode, useState } from "react";

import { AdminDialog } from "@/components/admin-dialog";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";

/**
 * Shown after a successful encrypted-store create or rotate-key call.
 * The key value lives only in this component's state — it is dropped
 * the moment the operator confirms the acknowledgement checkbox and
 * closes the dialog. Never written to localStorage, sessionStorage,
 * cookies, or the URL.
 *
 * The dialog is intentionally non-dismissible until the ack box is
 * ticked: a stray escape / outside-click must not bury the key.
 */
export function KeyRevealDialog({
  title,
  description,
  keyHex,
  acknowledgement = "I have saved this key securely.",
  guidance,
  onClose,
}: {
  title: string;
  description: ReactNode;
  keyHex: string;
  acknowledgement?: ReactNode;
  guidance?: ReactNode;
  onClose: () => void;
}) {
  const [ack, setAck] = useState(false);
  const [copyStatus, setCopyStatus] = useState<"idle" | "copied" | "failed">(
    "idle",
  );

  async function onCopy() {
    try {
      if (navigator.clipboard) {
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
    <AdminDialog
      open
      onClose={onClose}
      title={title}
      description={description}
      nonDismissible={!ack}
    >
      <div className="space-y-3" data-testid="key-reveal-dialog">
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
        {guidance ? (
          <div className="text-xs text-(--color-muted)">{guidance}</div>
        ) : null}
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
          <span>{acknowledgement}</span>
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
    </AdminDialog>
  );
}
