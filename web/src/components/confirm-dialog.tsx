import * as React from "react";
import { useState } from "react";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { describeApiError } from "@/lib/error-message";

export interface ConfirmDialogProps {
  title: string;
  description: React.ReactNode;
  body?: React.ReactNode;
  destructive?: boolean;
  confirmLabel: string;
  pendingLabel: string;
  onConfirm: () => unknown;
  onClose: () => void;
  /** When set, the confirm button is disabled. */
  disabled?: boolean;
  /** Test id applied to the confirm button (for Playwright selectors). */
  confirmTestId?: string;
}

export function ConfirmDialog({
  title,
  description,
  body,
  destructive,
  confirmLabel,
  pendingLabel,
  onConfirm,
  onClose,
  disabled,
  confirmTestId,
}: ConfirmDialogProps) {
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  async function handleConfirm() {
    setSubmitError(null);
    setPending(true);
    try {
      await onConfirm();
    } catch (err) {
      setSubmitError(describeApiError(err));
    } finally {
      setPending(false);
    }
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        {body}
        {submitError ? <Alert>{submitError}</Alert> : null}
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            variant={destructive ? "destructive" : "default"}
            disabled={disabled || pending}
            onClick={handleConfirm}
            data-testid={confirmTestId}
          >
            {pending ? pendingLabel : confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
