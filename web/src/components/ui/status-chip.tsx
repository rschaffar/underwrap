import * as React from "react";

import { cn } from "@/lib/utils";

export type ChipTone = "emerald" | "amber" | "sky" | "neutral";

const TONE: Record<ChipTone, string> = {
  emerald: "border-emerald-500/30 bg-emerald-500/10 text-emerald-300",
  amber: "border-amber-500/30 bg-amber-500/10 text-amber-300",
  sky: "border-sky-500/30 bg-sky-500/10 text-sky-300",
  neutral: "border-(--color-line) bg-(--color-surface-1) text-(--color-muted)",
};

/**
 * Small status pill shared by the entity tables (store lock state,
 * client-instance status, auto-create flag). One radius token
 * (`rounded-md`) and a fixed tone palette keep the chips visually
 * consistent across pages.
 */
export function StatusChip({
  tone,
  className,
  ...props
}: React.HTMLAttributes<HTMLSpanElement> & { tone: ChipTone }) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-md border px-2 py-0.5 text-xs",
        TONE[tone],
        className,
      )}
      {...props}
    />
  );
}
