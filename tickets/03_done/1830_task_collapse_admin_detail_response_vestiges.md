# Task: Collapse vestigial admin detail-response shapes

## Background

Ticket 1810 deleted the three admin detail-GET endpoints
(`/v1/admin/{stores/{name}, users/{id}, client-instances/{id}}`) and
the `/v1/admin/policy/put` endpoint. The corresponding
`Admin*DetailResponse` wire types stayed behind only because the
create/update endpoints still typed their JSON return as
`AdminFooDetailResponse`.

The result is a cluster of vestiges:

- `AdminUserDetailResponse` is byte-identical to `AdminUserListItem`
  (`crates/shared-protocol/src/service.rs:19, 32`).
- `AdminClientInstanceDetailResponse` differs from
  `AdminClientInstanceListItem` by one optional field,
  `current_credential_issued_at`, that the SPA never reads
  (`crates/shared-protocol/src/service.rs:251, 274`; SPA bridge at
  `web/src/pages/client-instances/page.tsx:25` literally
  zero-fills it as `undefined`).
- `AdminStoreDetailResponse` carries `dir` and `source_path` that the
  SPA explicitly zero-fills with empty strings in
  `web/src/pages/stores/page.tsx:19` because the dialogs that take
  "detail" never read them.
- `POST /v1/admin/policy/get` is the only endpoint that returns
  `PolicyDocument`, and the only caller is
  `web/src/components/policy-viewer-dialog.tsx` rendering
  `JSON.stringify(data.document, null, 2)`. The dialog already does
  `data.document.permissions.filter(...)` — i.e. all it actually
  uses from the wire is the permissions list.
  `POST /v1/admin/wrapper-permissions/list` returns the same
  permissions in a flatter envelope and is already shipped.

This is a single coherent cleanup: drop the detail-response wire
types, collapse to the list-item shape everywhere, and replace the
policy-get envelope by the existing wrapper-permissions list.

## Scope

One PR with four matching changes. They share the OpenAPI snapshot
and the regenerated `web/src/api/schema.ts`, so they have to land
together.

### Backend

1. **Drop `AdminUserDetailResponse`.** Update
   `handle_admin_user_create` and `handle_admin_user_update` to
   return `Json<AdminUserListItem>`. Delete the `user_detail` helper
   in `admin_api/users.rs:39` — call sites can take the existing
   `user_list_item` helper.

2. **Drop `AdminClientInstanceDetailResponse` + the
   `current_credential_issued_at` field.** Update
   `handle_admin_client_instance_create` and
   `handle_admin_client_instance_update` to return
   `Json<AdminClientInstanceListItem>`. Delete the
   `admin_client_instance_detail` helper in
   `admin_api/client_instances.rs:114`. The field is never surfaced
   by the SPA; deleting it shrinks the wire — if it ever ends up
   needed it can be added to the list item directly.

3. **Drop `AdminStoreDetailResponse` + the `dir` and `source_path`
   fields.** Update
   `handle_admin_store_create`, `handle_admin_store_update`,
   `handle_admin_store_unlock`, `handle_admin_store_lock`, and
   `handle_admin_store_rotate_key` to return JSON shapes built from
   `AdminStoreListItem`. Delete the `admin_store_detail` helper in
   `admin_api/stores/mod.rs:140`. Verify no other handler reads
   those two fields — they only exist because the deleted detail
   GET surfaced them.

4. **Delete `/v1/admin/policy/get`.** Remove
   `handle_admin_policy_get`, its
   `#[utoipa::path]` block, the `.routes(routes!(...))` entry, and
   `AdminPolicyGet{Request, Response}` from
   `shared-protocol/src/service.rs`. The `PolicyDocument` type is
   still in use by the runtime API and the storage layer; keep it.

### Frontend

5. **Drop the `toStoreDetail` and `toDetail` bridges.** In
   `web/src/pages/stores/page.tsx` and
   `web/src/pages/client-instances/page.tsx`, change the dialog
   props to consume the list-item type directly and feed list rows
   in unchanged. Remove the bridge functions.

6. **Rewire the policy viewer.** In
   `web/src/components/policy-viewer-dialog.tsx`, switch the
   `useQuery` call from `postPolicyGet({ store })` to
   `postWrapperPermissionsList({ store })`. Render
   `JSON.stringify({ permissions: data.permissions }, null, 2)` so
   the operator still sees a `PolicyDocument`-shaped JSON blob
   (just constructed on the FE). Update the writable-count badge to
   read `data.permissions` directly. Delete the `postPolicyGet` and
   `AdminPolicyGet*` exports from `web/src/api/client.ts`; the
   regenerated `schema.ts` won't have them anyway.

### OpenAPI + Schema

7. Run `cargo test --workspace`; the `openapi_export.rs` drift test
   regenerates `crates/backend-backendd/openapi.json`. Commit the
   result.

8. Run `npm run generate:api` so `web/src/api/schema.ts` picks up
   the new shapes. Commit if it changes (it will).

## Out of scope

- Any change to the runtime API (`runtime_api/`) or to
  `PolicyDocument` itself.
- Any change to `AdminStoreListItem`, `AdminUserListItem`, or
  `AdminClientInstanceListItem` *contents* beyond the field deletes
  called out above.
- Renaming the list-item types. Keep `AdminFooListItem`; do not
  rename to `AdminFoo` even though only one shape survives — the
  `ListItem` suffix matches the list-response field and is what
  every existing call site already reads.
- The `actions` / `usage` projection fields. Those are real,
  consumed by the SPA, and out of scope.

## Acceptance criteria

- [ ] `grep -rn "AdminStoreDetailResponse\|AdminUserDetailResponse\|AdminClientInstanceDetailResponse\|AdminPolicyGetRequest\|AdminPolicyGetResponse" crates/`
      returns zero hits.
- [ ] `grep -rn "AdminStoreDetailResponse\|AdminUserDetailResponse\|AdminClientInstanceDetailResponse\|AdminPolicyGetRequest\|AdminPolicyGetResponse" web/src/`
      returns zero hits.
- [ ] `grep -rn "fn admin_store_detail\|fn user_detail\|fn admin_client_instance_detail" crates/`
      returns zero hits.
- [ ] `grep -rn "toStoreDetail\|toDetail" web/src/` returns zero hits.
- [ ] `crates/backend-backendd/openapi.json` no longer mentions
      `/v1/admin/policy/get` or any of the deleted types.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` and
      `cargo clippy --all-targets --workspace -- -D warnings` pass.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes (TypeScript catches any
      stranded `AdminStoreDetailResponse` references the grep
      misses).
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.

## Notes for the implementer

- Land as one PR. The wire types, the SPA bridges, and the
  policy-viewer rewire all see the same regenerated `schema.ts`;
  splitting would create a window where the FE compiles against a
  schema that no longer matches the running backend.
- The `current_credential_issued_at` field disappears from the
  wire. If you want a paper trail, the field's last definition is
  in this ticket's history — there's no migration needed because no
  on-disk format references it.
- The policy viewer currently renders `data.document` as the JSON
  blob. To keep the operator-visible JSON shape stable
  (operators paste it into documentation / tickets), wrap the new
  permissions array in a fresh `{ permissions: [...] }` object
  before stringifying. The on-wire shape changes; the rendered
  blob does not.
- After the changes, `admin_store_list_item` is the sole projection
  helper for stores; consider renaming its body comments if they
  still refer to "list vs detail."
