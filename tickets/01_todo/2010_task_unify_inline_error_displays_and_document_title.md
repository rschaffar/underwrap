# Task: Unify inline error displays and extract `useDocumentTitle`

## Background

Two repeated idioms in the SPA:

- **Five pages** render their top-level fetch error as
  `<p role="alert" className="text-sm text-rose-300">{describeApiError(error)}</p>`
  while every dialog uses the shared `<Alert>` component. `Alert`
  already defaults `variant="destructive"` and sets `role="alert"`,
  so the inline paragraph reinvents the shadcn primitive
  (`web/src/components/ui/alert.tsx:6-15`).
- **Eight pages** carry an identical
  `useEffect(() => { document.title = "underwrap — …"; }, []);`
  block.

Source review:
`tickets/_review/web_admin_spa_2026-05-23.md` F-005, F-006.

## Scope

### Add a `useDocumentTitle` hook

Create `web/src/hooks/use-document-title.ts`:

```ts
import { useEffect } from "react";

export function useDocumentTitle(title: string): void {
  useEffect(() => {
    document.title = title;
  }, [title]);
}
```

### Convert page-level error paragraphs to `<Alert>`

In each file below, replace the inline
`<p role="alert" className="text-sm text-rose-300">…</p>` block
with `<Alert>{describeApiError(error)}</Alert>`:

- `web/src/pages/stores/page.tsx:57-59`
- `web/src/pages/users/page.tsx:64-66`
- `web/src/pages/client-instances/page.tsx:101-103`
- `web/src/pages/wrapper-permissions/page.tsx:181-183`
- `web/src/pages/secrets/page.tsx:165-167`

Drop the `import { describeApiError } from "@/lib/error-message";`
only if no other usage remains (all five pages keep it).

### Adopt the hook in every page that sets `document.title`

Convert the eight inline `useEffect` blocks to the new hook:

- `web/src/pages/login.tsx:33-35`
- `web/src/pages/bootstrap.tsx:44-46`
- `web/src/pages/profile.tsx:42-44`
- `web/src/pages/users/page.tsx:26-28`
- `web/src/pages/client-instances/page.tsx:45-47`
- `web/src/pages/stores/page.tsx:20-22`
- `web/src/pages/secrets/page.tsx:34-36`
- `web/src/pages/wrapper-permissions/page.tsx:33-35`

Drop the now-unused `useEffect` import from each page where this
was its only call.

## Out of scope

- Restyling `<Alert>` itself or adding new variants.
- Per-dialog error rendering (already uses `<Alert>`).
- Any change to `describeApiError`.

## Acceptance criteria

- [ ] `grep -rn 'role="alert"' web/src/pages` returns zero hits.
- [ ] `grep -rn 'document\.title' web/src/pages` returns zero hits.
- [ ] `grep -rn 'useDocumentTitle' web/src/pages` returns 8 hits.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes (the dashboard / page
      titles are exercised there).
