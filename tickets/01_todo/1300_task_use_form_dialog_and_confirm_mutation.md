# Task: `useFormDialog` + `useConfirmMutation` consolidation

Resolves: W-DLG-1, W-DLG-2; partially W-DLG-7.

## Problem

Every form-bearing dialog in the SPA repeats a five-line scaffold:

```ts
const qc = useQueryClient();
const [submitError, setSubmitError] = useState<string | null>(null);
const form = useForm<Values>({ resolver: zodResolver(schema), ... });
const mut = useMutation({
  mutationFn: ...,
  onSuccess: async () => { await qc.invalidateQueries(...); onClose(); },
  onError: (err) => setSubmitError(describeApiError(err)),
});
```

And every confirm-delete dialog repeats a near-identical
`useMutation` + `<ConfirmDialog onConfirm={() => mut.mutateAsync()}>`
shape.

Worse, the error-display pattern diverges across the family:

- `web/src/pages/client-instances/dialogs/create.tsx:71-86,204-206`
  and `edit.tsx:77-108,218-220` use `form.setError("root", ...)`.
- `web/src/pages/stores/dialogs/create.tsx:80,179` and
  `users/dialogs/create.tsx:68,141` use local `useState<string |
  null>(null)`.
- Libsecret dialogs use `setError(string)` with their own state.

`ConfirmDialog`'s own `pending` state shadows the caller's
`mut.isPending` (W-DLG-7): two state machines tracking the same fact.

## Affected dialogs (form-bearing)

- `web/src/pages/users/dialogs/{create,edit,reset-password}.tsx`
- `web/src/pages/stores/dialogs/{create,edit,unlock}.tsx`
- `web/src/pages/client-instances/dialogs/{create,edit}.tsx`
- `web/src/pages/secrets/dialogs/{create-resource,rename-resource,write}.tsx`
- `web/src/pages/wrapper-permissions/dialogs/create-pair.tsx`
- `web/src/pages/secrets/libsecret/dialogs/{create-collection,create-item,set-alias}.tsx`

## Affected dialogs (confirm-only)

- `web/src/pages/users/dialogs/delete.tsx`
- `web/src/pages/client-instances/dialogs/{delete,set-status}.tsx`
- `web/src/pages/stores/dialogs/lock.tsx`
- `web/src/pages/secrets/dialogs/delete-secret.tsx`
- `web/src/pages/secrets/libsecret/dialogs/{delete-item,delete-collection,delete-alias}.tsx`

## What to do

1. Add two hooks (under `web/src/hooks/` or `web/src/lib/`):
   ```ts
   useFormDialog<S extends z.ZodTypeAny>({
     schema, defaultValues, submit, invalidate, onSuccess,
   }): { form: UseFormReturn<...>, submit: () => void, error: string | null };

   useConfirmMutation({
     mutationFn, invalidateKeys, onClose,
   }): { confirm: () => Promise<void>, pending: boolean, error: string | null };
   ```
2. Pick **one** error-display pattern as canonical:
   `form.setError("root", { message: describeApiError(e) })` +
   `<Alert variant="destructive">{form.formState.errors.root?.message}</Alert>`
   inside `<DialogFooter>` or above it. The hook does the
   `form.setError` call internally on mutation error.
3. Convert every dialog in the lists above to use the hooks. Each
   should shrink to roughly:
   - schema definition
   - `useFormDialog` call (or `useConfirmMutation`)
   - JSX: `<AdminDialog>` (from 1340) + fields + footer with error
     alert + submit button bound to `submit`.
4. For `<ConfirmDialog>`: accept `pending: boolean` from the caller
   (or accept a `Promise<unknown>` callback and rely on the
   hook's `pending`), eliminate the duplicate state.

## Acceptance

- `cd web && npm test`
- `cd web && npm run build`
- `cd web && npm run test:e2e` (Playwright, inside `nix develop`)
- Manual walk-through: every create/edit/delete dialog closes on
  success, displays an error alert on failure, and shows a pending
  state during submission.
- Grep: no remaining `useState<string | null>` for submit errors
  in dialog files.
