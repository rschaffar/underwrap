import { type ReactNode, useState } from "react";

import { AdminDialog } from "@/components/admin-dialog";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export function KeyConfirmationDialog({
  title,
  description,
  keyHex,
  actionLabel,
  pendingLabel,
  pending,
  mutationAttempted,
  actionDisabledReason,
  submitError,
  onConfirm,
  onCancel,
  onDiscard,
}: {
  title: string;
  description: ReactNode;
  keyHex: string;
  actionLabel: string;
  pendingLabel: string;
  pending: boolean;
  mutationAttempted: boolean;
  actionDisabledReason?: string | null;
  submitError: string | null;
  onConfirm: () => void;
  onCancel: () => void;
  onDiscard: () => void;
}) {
  const [reentry, setReentry] = useState("");
  const [mismatch, setMismatch] = useState(false);
  const [discardWarning, setDiscardWarning] = useState(false);
  const [discardAck, setDiscardAck] = useState(false);
  const [copyStatus, setCopyStatus] = useState<"idle" | "copied" | "failed">(
    "idle",
  );

  async function copyKey() {
    try {
      if (!navigator.clipboard) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(keyHex);
      setCopyStatus("copied");
    } catch {
      setCopyStatus("failed");
    }
  }

  function confirmExactMatch() {
    if (reentry !== keyHex) {
      setMismatch(true);
      return;
    }
    setMismatch(false);
    setReentry("");
    onConfirm();
  }

  return (
    <AdminDialog
      open
      nonDismissible
      onClose={() => {}}
      title={title}
      description={description}
    >
      <div className="space-y-4" data-testid="key-confirmation-dialog">
        <Alert variant="warning">
          Save this key in an external password manager or vault before
          continuing. Exact re-entry proves only that this browser comparison
          matched; it does not prove response delivery, external custody, or
          independent recoverability. The backend does not persist the key.
        </Alert>
        <div className="space-y-2">
          <p
            id="generated-store-key-label"
            className="text-sm font-medium text-(--color-fg)"
          >
            Generated master key (hex)
          </p>
          <pre
            aria-labelledby="generated-store-key-label"
            data-testid="generated-store-key"
            className="overflow-x-auto rounded-md border border-(--color-line) bg-(--color-surface-1) p-3 font-mono text-xs text-(--color-fg)"
          >
            {keyHex}
          </pre>
          <div className="flex items-center gap-2">
            <Button type="button" size="sm" variant="ghost" onClick={copyKey}>
              Copy to clipboard
            </Button>
            <span className="text-xs text-(--color-muted)" aria-live="polite">
              {copyStatus === "copied"
                ? "Copied."
                : copyStatus === "failed"
                  ? "Clipboard unavailable; copy the displayed value manually."
                  : ""}
            </span>
          </div>
        </div>
        <div className="space-y-2">
          <Label htmlFor="store-key-reentry">
            Re-enter the exact generated key without trimming
          </Label>
          <Input
            id="store-key-reentry"
            data-testid="store-key-reentry"
            value={reentry}
            onChange={(event) => {
              setReentry(event.target.value);
              setMismatch(false);
            }}
            autoComplete="off"
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
            aria-invalid={mismatch}
            aria-describedby={mismatch ? "store-key-mismatch" : undefined}
            className="font-mono text-xs"
          />
          {mismatch ? (
            <p
              id="store-key-mismatch"
              className="text-xs text-(--color-danger)"
              role="alert"
            >
              The re-entered key does not exactly match. No mutation request was
              sent.
            </p>
          ) : null}
        </div>
        {submitError ? <Alert>{submitError}</Alert> : null}
        {mutationAttempted ? (
          <Alert variant="warning">
            The browser attempted to submit the mutation, and it may have
            reached the backend. An error or lost response cannot prove that no
            data changed. Retain this key while reconciling the Stores list;
            retry uses this same displayed key when the source remains eligible.
          </Alert>
        ) : null}
        {actionDisabledReason ? (
          <Alert variant="warning">{actionDisabledReason}</Alert>
        ) : null}
        {discardWarning ? (
          <div className="space-y-3 rounded-md border border-amber-500/40 p-3">
            <Alert variant="warning">
              This key may already protect committed or recoverable data.
              Discarding the only displayed copy can make that data unavailable.
            </Alert>
            <label className="flex items-start gap-2 text-xs text-(--color-muted)">
              <input
                type="checkbox"
                checked={discardAck}
                onChange={(event) => setDiscardAck(event.target.checked)}
              />
              <span>I understand the retained key may still be required.</span>
            </label>
            <Button
              type="button"
              variant="destructive"
              disabled={!discardAck || pending}
              onClick={onDiscard}
            >
              Discard key
            </Button>
          </div>
        ) : null}
      </div>
      <DialogFooter>
        {mutationAttempted ? (
          <Button
            type="button"
            variant="ghost"
            disabled={pending}
            onClick={() => setDiscardWarning(true)}
          >
            Discard retained key…
          </Button>
        ) : (
          <Button type="button" variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
        )}
        <Button
          type="button"
          disabled={pending || Boolean(actionDisabledReason)}
          onClick={confirmExactMatch}
        >
          {pending
            ? pendingLabel
            : mutationAttempted
              ? `Retry ${actionLabel}`
              : actionLabel}
        </Button>
      </DialogFooter>
    </AdminDialog>
  );
}
