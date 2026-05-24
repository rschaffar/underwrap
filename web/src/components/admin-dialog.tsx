import * as React from "react";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

export interface AdminDialogProps {
  open: boolean;
  onClose: () => void;
  title: React.ReactNode;
  description?: React.ReactNode;
  children: React.ReactNode;
  /**
   * When `true`, Escape, outside-click, and pointer-down-outside are
   * suppressed. Use for one-shot key-reveal flows that require an
   * explicit acknowledgement before they can be closed.
   */
  nonDismissible?: boolean;
}

/**
 * Standard admin SPA dialog shell. Wraps Radix `Dialog` with the
 * `onOpenChange` ↔ `onClose` translation so every page-level dialog
 * stops re-implementing the same five lines.
 */
export function AdminDialog({
  open,
  onClose,
  title,
  description,
  children,
  nonDismissible,
}: AdminDialogProps) {
  const block = (e: Event) => {
    if (nonDismissible) e.preventDefault();
  };
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => !o && !nonDismissible && onClose()}
    >
      <DialogContent onEscapeKeyDown={block} onInteractOutside={block}>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          {description ? (
            <DialogDescription>{description}</DialogDescription>
          ) : null}
        </DialogHeader>
        {children}
      </DialogContent>
    </Dialog>
  );
}
