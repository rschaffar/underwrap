import * as React from "react";
import { useNavigationLock } from "@/hooks/use-navigation-lock";

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
   * suppressed, route navigation is blocked, and unload prompts are armed.
   * Use for pending mutations and acknowledgement-gated key custody.
   * Explicit footer actions must independently respect their pending state.
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
  useNavigationLock(open && Boolean(nonDismissible));
  const block = (e: Event) => {
    if (nonDismissible) e.preventDefault();
  };
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => !o && !nonDismissible && onClose()}
    >
      <DialogContent
        closeDisabled={nonDismissible}
        onEscapeKeyDown={block}
        onInteractOutside={block}
      >
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
