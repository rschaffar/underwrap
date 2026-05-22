# Task: Add `<Alert>` primitive and replace inline alert divs

Parent: `1500_story_web_spa_refinement.md`
Resolves: F-021 (review report `web_spa_2026-05-22.md`)

## Background

The pattern

```tsx
<p
  role="alert"
  className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
>
  {message}
</p>
```

is copy-pasted 25+ times across the SPA. The amber/`text-amber-200`
warning variant and the emerald `role="status"` success variant
appear several times each as well.

Representative sites:

- `web/src/pages/secrets.tsx:236-243, 291-294, 712-717, 920-928,
  1106-1112, 1361-1366`
- `web/src/pages/wrapper-permissions.tsx:293-300, 359-362,
  785-790, 884-889, 1088-1094`
- `web/src/pages/stores.tsx:124-127, 468-473, 591-597, 642-646,
  875-882, 925-932, 947-952, 1070-1077`
- `web/src/pages/client-instances.tsx:124-128, 367-374, 521-527,
  625-631, 685-691`
- `web/src/pages/users.tsx:83-86, 341-347, 437-443, 542-548,
  605-611`
- `web/src/pages/profile.tsx:140-146, 148-156` (the success
  variant)
- `web/src/pages/login.tsx:114-121`
- `web/src/pages/bootstrap.tsx:144-151`
- `web/src/components/policy-viewer-dialog.tsx:91-96`

Plus the amber-warning variants embedded in `stores.tsx:1046-1058`
(blocked-by-mapping) and `1054-1058` (requires-force) and the
amber-warning variants in `secrets.tsx:1308-1357` (delete blockers)
and `wrapper-permissions.tsx:862-880` (delete pair blockers).

A reader has to verify the className spelling at every site, and
any restyling becomes a 30-site rewrite.

## Scope

- Create `web/src/components/ui/alert.tsx`:

  ```tsx
  import * as React from "react";
  import { cva, type VariantProps } from "class-variance-authority";
  import { cn } from "@/lib/utils";

  const alertVariants = cva(
    "rounded-md border px-3 py-2 text-xs",
    {
      variants: {
        variant: {
          destructive: "border-rose-500/30 bg-rose-500/10 text-rose-300",
          warning: "border-amber-500/30 bg-amber-500/10 text-amber-200",
          success: "border-emerald-500/30 bg-emerald-500/10 text-emerald-200",
        },
      },
      defaultVariants: { variant: "destructive" },
    },
  );

  export interface AlertProps
    extends React.HTMLAttributes<HTMLDivElement>,
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
  ```

- Replace every inline site listed in Background. The body of
  each call becomes either a plain string (most submit-error sites)
  or the structured content that was previously inside the `<p>`
  (e.g., `secrets.tsx:1308-1357` keeps its nested list, just
  wrapped by `<Alert variant="warning">`). Multi-line warning
  bodies that currently use `<p>` may change to `<div>`; that's
  fine.

- Existing tests that assert on `role="alert"` text keep passing —
  the destructive variant preserves the role. The
  `policy-viewer-dialog.test.tsx:102-114` and
  `bootstrap.test.tsx:91-114` `findByRole("alert")` lookups
  continue to find the new component.

- Existing tests that assert specific Tailwind classes (none
  found in the suite, but verify with
  `rg -n 'border-rose-500/30' web/src/**/*.test.tsx` before
  closing) would need updating. None expected.

## Acceptance criteria

- [ ] `rg -n 'role="alert"[[:space:]]*\n[[:space:]]*className="rounded-md border border-rose-500' web/src/`
      returns nothing.
- [ ] `rg -n 'border-rose-500/30 bg-rose-500/10' web/src/`
      returns at most the `alert.tsx` file itself.
- [ ] `rg -n 'border-amber-500/30 bg-amber-500/10' web/src/`
      returns at most the `alert.tsx` file itself.
- [ ] `rg -n 'border-emerald-500/30 bg-emerald-500/10' web/src/`
      returns at most the `alert.tsx` file itself.
- [ ] `cd web && npm test` passes (no test wording changes
      expected; the rendered text is identical).
- [ ] `cd web && npm run build` passes.
