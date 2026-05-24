import * as React from "react";

import { AdminDialog } from "@/components/admin-dialog";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";

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
  /** Owned by the caller (typically `useConfirmMutation`). */
  pending: boolean;
  /** Owned by the caller. Rendered as the standard `<Alert>`. */
  submitError: string | null;
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
  pending,
  submitError,
  confirmTestId,
}: ConfirmDialogProps) {
  return (
    <AdminDialog open onClose={onClose} title={title} description={description}>
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
          onClick={() => void onConfirm()}
          data-testid={confirmTestId}
        >
          {pending ? pendingLabel : confirmLabel}
        </Button>
      </DialogFooter>
    </AdminDialog>
  );
}
