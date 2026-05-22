# Task: Add `<Select>` primitive and migrate inline selects

Parent: `1500_story_web_spa_refinement.md`
Resolves: F-020 (review report `web_spa_2026-05-22.md`)

## Background

Inline `<select>` elements carry the same Tailwind class string in
six call sites:

`"flex h-9 [w-full] rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 text-sm text-(--color-fg)"`

with minor variations (some sites add `w-full`, the
secrets-store and wrappers-store selects skip it for the
inline-label layout). Sites:

- `web/src/pages/secrets.tsx:272-283` — store selector
- `web/src/pages/wrapper-permissions.tsx:339-351` — store selector
- `web/src/pages/stores.tsx:432-438` — backend kind (`plain`/`encrypted`)
- `web/src/pages/client-instances.tsx:102-112` — status filter
- `web/src/pages/client-instances.tsx:311-322` — create-dialog
  store dropdown
- `web/src/pages/client-instances.tsx:326-334` — create-dialog
  status dropdown
- `web/src/pages/client-instances.tsx:472-484` — edit-dialog store
  dropdown

There is no `<Select>` primitive sibling to `<Input>` /
`<Label>` / `<Button>` under `web/src/components/ui/`.

## Scope

- Create `web/src/components/ui/select.tsx`:

  ```tsx
  import * as React from "react";
  import { cn } from "@/lib/utils";

  export type SelectProps = React.SelectHTMLAttributes<HTMLSelectElement>;

  export const Select = React.forwardRef<HTMLSelectElement, SelectProps>(
    ({ className, children, ...props }, ref) => (
      <select
        ref={ref}
        className={cn(
          "flex h-9 w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 text-sm text-(--color-fg)",
          "focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-(--color-accent)/40",
          "disabled:cursor-not-allowed disabled:opacity-50",
          className,
        )}
        {...props}
      >
        {children}
      </select>
    ),
  );
  Select.displayName = "Select";
  ```

  Stay native-`<select>`; do not pull in `@radix-ui/react-select`
  unless someone asks for the richer popover UI.

- Migrate the six call sites. Sites that currently omit `w-full`
  (the inline-label store selectors in secrets / wrapper-permissions
  headers) override with `className="w-auto"` or pass
  `className="ml-1 …"`-style classes — verify the resulting layout
  matches the current screenshot.

- `wrapper-permissions.test.tsx` and `client-instances.test.tsx`
  drive selects via `user.selectOptions(screen.getByLabelText(…),
  value)`. Both already work against native `<select>` so nothing
  in the tests changes.

## Acceptance criteria

- [ ] `rg -n '<select' web/src/pages/` returns nothing.
- [ ] `web/src/components/ui/select.tsx` exists and is imported
      by every former call site.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes — the Playwright
      specs touch the wrappers-store and client-instances-store
      selects via `getByRole("combobox")` / `getByLabel`.
