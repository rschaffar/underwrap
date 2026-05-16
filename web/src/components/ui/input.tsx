import * as React from "react";

import { cn } from "@/lib/utils";

export type InputProps = React.InputHTMLAttributes<HTMLInputElement>;

const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className, type, ...props }, ref) => {
    return (
      <input
        type={type}
        ref={ref}
        className={cn(
          "flex h-9 w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 text-sm text-(--color-fg) shadow-sm transition-colors",
          "placeholder:text-(--color-muted)/70",
          "focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-(--color-accent)/40",
          "disabled:cursor-not-allowed disabled:opacity-50",
          "aria-[invalid=true]:border-rose-500/60 aria-[invalid=true]:focus-visible:ring-rose-500/40",
          className,
        )}
        {...props}
      />
    );
  },
);
Input.displayName = "Input";

export { Input };
