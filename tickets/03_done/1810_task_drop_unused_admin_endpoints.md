# Task: Drop unused admin API endpoints

## Background

A pass over the admin surface against the SPA's actual call sites
(`web/src/api/client.ts` exports vs. real consumers under
`web/src/pages/`, `web/src/components/`) turned up seven handlers that
exist in the backend, ship typed wrappers in `client.ts`, appear in
the generated OpenAPI spec — and have zero callers. They survived
the 1700-series framework migration as dead weight.

Greenfield posture: the right move is to delete them before first
release rather than keep them around "for symmetry." A few are also
mild footguns (e.g. `policy/put` can clobber concurrent
wrapper-permission edits if anyone wires it up later by mistake;
`resources/blob/{get,put}` exposes the JCS-canonical blob, which the
SPA must never touch directly now that we have per-key secret
endpoints).

Verified unused by grepping `web/src/` for the exported names —
matches only in `api/client.ts` (the wrapper) and `api/schema.ts`
(the generated types), no real call sites.

## Scope

One PR that removes the endpoints below and their downstream
artifacts. No SPA changes should be required; the typed client
exports go away but no `*.tsx` imports them.

### Endpoints to remove

| Method + path | Handler |
|---|---|
| `GET /v1/admin/stores/{id}` | `admin_api/stores/crud.rs::handle_admin_store_detail` |
| `GET /v1/admin/users/{id}` | `admin_api/users.rs::handle_admin_user_detail` |
| `GET /v1/admin/client-instances/{id}` | `admin_api/client_instances.rs::handle_admin_client_instance_detail` |
| `POST /v1/admin/policy/put` | `admin_api/policy.rs::handle_admin_policy_put` |
| `POST /v1/admin/resources/blob/get` | `admin_api/resources.rs::handle_admin_resource_blob_get` |
| `POST /v1/admin/resources/blob/put` | `admin_api/resources.rs::handle_admin_resource_blob_put` |
| `GET /v1/admin/ui/overview` | `admin_api/overview.rs::handle_admin_ui_overview` |

`POST /v1/admin/policy/get` is **kept** — it is used by
`web/src/components/policy-viewer-dialog.tsx` for the read-only
policy document view.

### Backend deletions

For each removed handler:

1. Drop the handler function and its `#[utoipa::path(...)]` block.
2. Remove its `.routes(routes!(...))` line from the subdomain's
   `openapi_router()`.
3. Drop the now-unused request/response types from
   `crates/shared-protocol/src/service.rs` (and any
   `entities.rs`/`policy.rs` types that survive only because of these
   endpoints — e.g. `AdminUiOverview*`, `AdminResourceBlob{Get,Put}*`,
   `AdminStoreDetailResponse`, `AdminUserDetailResponse`,
   `AdminClientInstanceDetailResponse`, `AdminPolicyPutRequest`).
4. Drop unit tests that exercise the deleted handler. Tests that
   incidentally call the endpoint as part of a fixture flow must
   move to the corresponding list/create/update endpoint.

For `overview.rs`: every route in that file is on the deletion
list, so the whole module goes — delete the file and drop the
`pub(crate) mod overview;` declaration plus the `.merge(overview::
openapi_router())` line in `admin_api/mod.rs::build_protected_router`.

The `audit.rs` log_admin_event "overview" callers (if any) go away
with the file. No other subdomain references overview types.

### Frontend deletions

In `web/src/api/client.ts`:

- Delete the exported wrappers `fetchStore`, `fetchUser`,
  `fetchClientInstance`, `fetchUiOverview` (if it exists),
  `postResourceBlobGet`, `postResourceBlobPut`, `postPolicyPut`.
- Delete the type re-exports for any `Admin*` names that become
  unused once those wrappers are gone.

Regenerate `web/src/api/schema.ts` from the new OpenAPI document
(the project already has a generation step — re-run whatever
`web/` uses; do **not** hand-edit `schema.ts`).

### OpenAPI drift test

`crates/backend-backendd/tests/openapi_export.rs` snapshots the
generated spec. Update the snapshot in the same PR. This is the
one place where a regenerate is the intended action.

## Out of scope

- Any of the kept endpoints. In particular: do not touch
  `/v1/admin/policy/get`, the granular
  `/v1/admin/wrapper-permissions/*` endpoints, the
  `/v1/admin/secret/*` family, or the `/v1/admin/pairs/*` family.
- Restructuring the remaining admin subdomain layout. That is the
  follow-up ticket 1820.
- Changing the runtime API (`runtime_api/`) at all.
- Replacing the deleted list/get pairs with a different shape (e.g.
  embedding more data in list responses). List responses already
  carry the fields the SPA needs; if a future page wants per-row
  detail, it can be added back deliberately at that time.

## Acceptance criteria

- [ ] None of the seven endpoints in the table above appear in the
      generated `openapi.json` snapshot.
- [ ] `crates/backend-backendd/src/admin_api/overview.rs` no longer
      exists and is removed from `admin_api/mod.rs`.
- [ ] `grep -rn "handle_admin_store_detail\|handle_admin_user_detail\|handle_admin_client_instance_detail\|handle_admin_policy_put\|handle_admin_resource_blob_get\|handle_admin_resource_blob_put\|handle_admin_ui_overview" crates/`
      returns zero hits.
- [ ] `grep -rn "fetchStore\b\|fetchUser\b\|fetchClientInstance\b\|fetchUiOverview\|postResourceBlobGet\|postResourceBlobPut\|postPolicyPut" web/src/`
      returns zero hits.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` and
      `cargo clippy --all-targets --workspace -- -D warnings` pass.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run test:e2e` passes (inside `nix develop`).
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.
- [ ] The OpenAPI drift snapshot is regenerated in the same commit
      so `openapi_export.rs` stays green.

## Notes for the implementer

- Land as one PR, not module-by-module. Half-deleted endpoints with
  partial SPA cleanup is exactly the parallel-with-legacy
  intermediate state we avoid.
- When deleting `policy/put`, double-check that
  `crates/shared-protocol/src/service.rs::AdminPolicyPutRequest` is
  not used by any other handler before deleting it — it should not
  be.
- For each deleted handler, follow the trail: handler →
  `#[utoipa::path]` → `routes!()` registration → wire types →
  `client.ts` wrapper → `schema.ts` regen. Missing any one of these
  leaves a stranded type or a 404 export.
- After regenerating `schema.ts`, scan for any `Admin*` type that no
  longer has a producer — those should disappear automatically with
  the regen, but spot-check the diff.
