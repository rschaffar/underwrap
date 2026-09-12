import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

const alertVariants = cva("rounded-md border px-3 py-2 text-xs", {
  variants: {
    variant: {
      destructive: "border-rose-500/30 bg-rose-500/10 text-(--color-danger)",
      warning: "border-amber-500/30 bg-amber-500/10 text-(--color-warning)",
      success: "border-emerald-500/30 bg-emerald-500/10 text-(--color-success)",
    },
  },
  defaultVariants: { variant: "destructive" },
});

export interface AlertProps
  extends
    React.HTMLAttributes<HTMLDivElement>,
    VariantProps<typeof alertVariants> {}

export const Alert = React.forwardRef<HTMLDivElement, AlertProps>(
  ({ className, variant, role, ...props }, ref) => (
    <div
      ref={ref}
      role={role ?? (variant === "success" ? "status" : "alert")}
      className={cn(alertVariants({ variant }), className)}
      {...props}
    />
  ),
);
Alert.displayName = "Alert";
