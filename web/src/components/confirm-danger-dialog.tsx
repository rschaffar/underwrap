import * as React from "react";
import { AlertTriangle } from "lucide-react";

import { AdminDialog } from "@/components/admin-dialog";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";

export interface ConfirmDangerDialogProps {
  title: string;
  description: React.ReactNode;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * Confirmation gate for an irreversible-feeling or security-sensitive
 * choice. Warning palette (amber/rose), cancel focused by default so a
 * stray Enter does not arm the dangerous option. Escape / outside-click
 * count as cancel. Mount conditionally (`{open ? <…/> : null}`) like the
 * other admin dialogs rather than toggling an `open` prop.
 */
export function ConfirmDangerDialog({
  title,
  description,
  confirmLabel,
  onConfirm,
  onCancel,
}: ConfirmDangerDialogProps) {
  return (
    <AdminDialog
      open
      onClose={onCancel}
      title={
        <span className="inline-flex items-center gap-2 text-(--color-warning)">
          <AlertTriangle className="h-4 w-4" aria-hidden />
          {title}
        </span>
      }
      description={description}
    >
      <DialogFooter>
        <Button
          type="button"
          variant="ghost"
          autoFocus
          onClick={onCancel}
          data-testid="confirm-danger-cancel"
        >
          Cancel
        </Button>
        <Button
          type="button"
          variant="destructive"
          onClick={onConfirm}
          data-testid="confirm-danger-confirm"
        >
          {confirmLabel}
        </Button>
      </DialogFooter>
    </AdminDialog>
  );
}
