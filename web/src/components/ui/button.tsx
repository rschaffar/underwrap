import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium ring-offset-2 transition-colors focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-(--color-accent) disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        default:
          "bg-(--color-accent)/15 text-(--color-accent) hover:bg-(--color-accent)/25 border border-(--color-accent)/30",
        secondary:
          "bg-(--color-surface-1) text-(--color-fg) hover:bg-(--color-surface-2) border border-(--color-line)",
        ghost:
          "text-(--color-muted) hover:bg-(--color-surface-1) hover:text-(--color-fg)",
        destructive:
          "bg-rose-500/15 text-(--color-danger) hover:bg-rose-500/25 border border-rose-500/30",
        outline:
          "border border-(--color-line) bg-transparent text-(--color-fg) hover:bg-(--color-surface-1)",
      },
      size: {
        default: "h-9 px-4 py-2",
        sm: "h-8 px-3 text-xs",
        lg: "h-10 px-5 text-base",
        icon: "h-9 w-9",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

export interface ButtonProps
  extends
    React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return (
      <Comp
        className={cn(buttonVariants({ variant, size }), className)}
        ref={ref}
        {...props}
      />
    );
  },
);
Button.displayName = "Button";

export { Button, buttonVariants };
