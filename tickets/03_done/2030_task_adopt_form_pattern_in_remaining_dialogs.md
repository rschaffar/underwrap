# Task: Convert client-instances and create-pair dialogs to `useForm + zodResolver + <Form>`

## Background

Three dialogs bypass the SPA's shared form scaffolding and manage
their own validation, error placement, and accessibility wiring by
hand:

- `web/src/pages/client-instances/dialogs/create.tsx:33-38`
- `web/src/pages/client-instances/dialogs/edit.tsx:35-41`
- `web/src/pages/wrapper-permissions/dialogs/create-pair.tsx:38-49`

Every other admin dialog uses the
`useForm({ resolver: zodResolver(schema) })` + `<Form>` +
`<FormField>` + `<FormMessage>` pattern (see
`web/src/pages/users/dialogs/create.tsx:60-67`,
`web/src/pages/secrets/dialogs/create-resource.tsx:56-59`). The
hand-rolled dialogs miss `aria-invalid` / `aria-describedby`
wiring (the cert-PEM textareas have no `<FormMessage>`) and
re-implement submit gating
(`displayName.trim() !== "" && storeId !== "" && trimmedCert !== ""`).

Source review:
`tickets/_review/web_admin_spa_2026-05-23.md` F-007.

## Scope

For each of the three dialogs, replace the raw
`useState` + `<Label>` + `<Input>` + `<textarea>` scaffolding with
the shared pattern.

### `client-instances/dialogs/create.tsx`

zod schema:

```ts
const schema = z.object({
  display_name: z.string().trim().min(1, "Display name is required."),
  current_store_id: z.string().min(1, "Pick a target store."),
  status: z.enum(["active", "disabled"]),
  cert_pem: z.string().trim().min(1, "Certificate PEM is required."),
  auto_create_wrapper_assets: z.boolean(),
});
```

`useForm` defaults mirror the current `useState` defaults. The
cert-PEM textarea becomes a `<FormControl>` child of a
`<FormField>`. Drop the hand-rolled `canSubmit` computation; rely
on `form.formState.isValid` (or just let zod reject on submit).

### `client-instances/dialogs/edit.tsx`

Same shape, except every field is optional and `cert_pem` keeps
its "leave blank to keep existing cert" semantics. The submit
payload still only includes changed fields — keep that logic in
the submit handler, fed from `form.formState.dirtyFields`.

### `wrapper-permissions/dialogs/create-pair.tsx`

zod schema:

```ts
const schema = z
  .object({
    wrapper: z.string().trim().min(1, "Wrapper name is required."),
    mode: z.enum(["existing", "new"]),
    resource_id: z.string().optional(),
    new_resource_name: z.string().trim().optional(),
  })
  .refine((v) => v.mode !== "existing" || !!v.resource_id, {
    path: ["resource_id"],
    message: "Pick an existing resource.",
  })
  .refine((v) => v.mode !== "new" || !!v.new_resource_name, {
    path: ["new_resource_name"],
    message: "Enter a name for the new resource.",
  });
```

The "creating new vs picking existing" radio becomes
`form.watch("mode")` based. The chained
`createResource.mutateAsync` → `createPair.mutateAsync` flow stays
inside the submit handler.

### Drop the `submitError` `useState`

Replace with `form.setError("root", { message: describeApiError(e) })`
in the catch arm, and render
`form.formState.errors.root?.message` via an `<Alert>` block
above the footer.

## Out of scope

- Refactoring the pair-creation backend flow (still two POSTs).
- Changing what gets sent on the wire (same payloads).
- Touching any other dialog. (Stores/users/secrets dialogs
  already use the pattern.)

## Acceptance criteria

- [ ] `grep -n "useState<" web/src/pages/client-instances/dialogs/create.tsx
      web/src/pages/client-instances/dialogs/edit.tsx
      web/src/pages/wrapper-permissions/dialogs/create-pair.tsx`
      returns no hits for input-field state (only `useState` for
      truly local UI state — e.g. radio mode selection — is
      acceptable; submit error state is not).
- [ ] Each of the three dialogs imports `useForm` and renders at
      least one `<FormField>`.
- [ ] The cert-PEM textarea in both client-instance dialogs is
      wrapped by `<FormControl>` (yields proper `aria-invalid` /
      `aria-describedby` wiring).
- [ ] Playwright tests for `client-instances.spec.ts` (create +
      edit flows) and the create-pair path in `wrappers.spec.ts`
      pass.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes.
