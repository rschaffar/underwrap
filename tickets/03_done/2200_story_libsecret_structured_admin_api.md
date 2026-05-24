# Story: Structured libsecret admin API

## Background

See `epic_libsecret_structured_admin.md` for context. This story adds
the new structured admin endpoints for libsecret. After it lands, the
flat libsecret endpoints still exist (the SPA hasn't switched yet); the
deletion of those endpoints happens in
`2300_story_retire_flat_libsecret_bridge.md`.

## Goal

Operators (and the admin SPA) can manage the libsecret tree of a
resource through endpoints that mirror the Secret Service spec
concepts: a Service-level alias table, a set of collections, items
inside each collection.

## End state

- All operations on libsecret content go through structured endpoints.
- Each handler is a single atomic load-modify-save against the resource
  blob (same pattern as `secret_entries.rs`).
- Wire types live in `shared-protocol::service` so the OpenAPI document
  describes them.
- Each operation emits a spec-faithful audit event.
- Integration tests cover the full CRUD surface plus invariant
  enforcement.

## Wire surface

All paths are rooted at `/v1/admin/stores/{sid}/resources/{rid}/libsecret`.

### Tree read

```
GET /v1/admin/stores/{sid}/resources/{rid}/libsecret
  → AdminLibsecretTreeResponse {
      aliases:     BTreeMap<String, String>,           // name → collection_id
      collections: Vec<AdminLibsecretCollectionView>,
    }

AdminLibsecretCollectionView {
  id, label, created, modified,
  items: Vec<AdminLibsecretItemView>,
}

AdminLibsecretItemView {
  id, label, content_type, attributes: BTreeMap<String, String>,
  created, modified,
}
```

Item secret bytes are intentionally absent from the tree view.

### Item secret read

```
GET .../libsecret/collections/{cid}/items/{iid}/secret
  → AdminLibsecretItemSecretResponse { secret_b64: String }
```

### Collection CRUD

```
POST   .../libsecret/collections
  body: { id?: String, label: String }
  → AdminLibsecretCollectionCreateResponse { id }

PATCH  .../libsecret/collections/{cid}
  body: { label?: String }
  → AdminOkResponse

DELETE .../libsecret/collections/{cid}
  → AdminLibsecretCollectionDeleteResponse {
      existed: bool,
      aliases_scrubbed: Vec<String>,   // alias names that pointed at this collection
    }
```

`DELETE` scrubs any aliases pointing at the deleted collection and
reports their names; the scrub is a single atomic blob save together
with the collection removal.

### Item CRUD

```
POST   .../libsecret/collections/{cid}/items
  body: {
    id?: String,
    label: String,
    content_type: String,
    attributes: BTreeMap<String, String>,
    secret_b64: String,
  }
  → AdminLibsecretItemCreateResponse { id }

PATCH  .../libsecret/collections/{cid}/items/{iid}
  body: {
    label?: String,
    content_type?: String,
    attributes?: BTreeMap<String, String>,
    secret_b64?: String,
  }
  → AdminOkResponse

DELETE .../libsecret/collections/{cid}/items/{iid}
  → AdminLibsecretItemDeleteResponse { existed: bool }
```

### Alias CRUD

```
PUT    .../libsecret/aliases/{name}
  body: { collection_id: String }
  → AdminOkResponse

DELETE .../libsecret/aliases/{name}
  → AdminLibsecretAliasDeleteResponse { existed: bool }
```

`PUT` returns `404` if `collection_id` does not name an existing
collection.

## Server-side rules

- Ids (collection id, item id, alias name) match `[A-Za-z0-9._-]+`,
  max length 128. Leading `_` is reserved.
- `created` is set on POST; `modified` is bumped server-side on every
  successful mutation. Clients never send either.
- Each handler is one load → mutate → `save_resource_blob` cycle. The
  storage layer's atomic-write contract gives transactional semantics
  for free.
- Collection delete cascades to its items (whole `Collection` struct
  removed in one save) and scrubs alias entries pointing at it.
- Alias PUT validates target collection existence.
- All endpoints require the existing per-store admin access check; per
  the comment in `secrets.rs`, admin actions are not gated by the
  per-resource `writable` flag.

## Audit events

New event kinds, recorded via `log_admin_event` with action names:

- `libsecret.collection.create`
- `libsecret.collection.update`
- `libsecret.collection.delete`
- `libsecret.item.create`
- `libsecret.item.update`
- `libsecret.item.set_secret` (when PATCH includes `secret_b64`)
- `libsecret.alias.set`
- `libsecret.alias.delete`

Each event includes `(store, resource_id, ...identifiers...)` plus
`result=success|failure`. Item secret bytes are never logged.

## File touchpoints

- `crates/shared-protocol/src/service.rs` — new wire types listed above.
- `crates/backend-backendd/src/admin_api/libsecret/` — new module,
  one file per resource (`tree.rs`, `collections.rs`, `items.rs`,
  `aliases.rs`), each ending with `#[cfg(test)] mod tests;`. Helpers
  (load blob, validate id) shared via the existing crate test_support
  pattern.
- `crates/backend-backendd/src/admin_api/mod.rs` — wire the new
  module into `openapi_router()`.
- `crates/backend-backendd/src/openapi.rs` — register new schemas /
  routes.
- `crates/backend-backendd/src/admin_api/audit.rs` — add the new event
  variants.
- `crates/integration-tests/src/tests/admin_libsecret.rs` — new file
  exercising the endpoints end-to-end.

## Out of scope

- Removing the flat libsecret endpoints. Both surfaces coexist until
  the SPA has migrated, then `2300` retires the flat side.
- Changing the daemon ↔ secretsd / daemon ↔ backend runtime paths.
- SPA UI work.
- Any change to `ResourceBlob` / `LibsecretSection` shapes.

## Acceptance criteria

- [ ] All wire types defined in `shared-protocol::service` with utoipa
      schemas registered.
- [ ] All endpoints listed above implemented and routed under
      `/v1/admin/stores/{sid}/resources/{rid}/libsecret`.
- [ ] Each handler performs exactly one `load_resource_blob` →
      mutate → `save_resource_blob` cycle.
- [ ] Collection DELETE atomically scrubs aliases that point at it and
      returns their names in the response.
- [ ] Alias PUT returns 404 when `collection_id` is unknown.
- [ ] Id validation rejects leading `_`, `/`, and over-length values
      with a structured error.
- [ ] Audit events listed above are emitted with `result=success` or
      `result=failure` as appropriate; no event carries secret material.
- [ ] Integration tests cover: tree read, collection CRUD lifecycle,
      item CRUD lifecycle (incl. secret read), alias PUT/DELETE, alias
      PUT with unknown collection (404), collection DELETE scrubs
      aliases, id validation rejections.
- [ ] OpenAPI document includes the new endpoints.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes (sanity, even
      though VM coverage of admin endpoints is light).

## Notes for the implementer

- The handlers are small. Resist factoring premature shared helpers
  beyond an id validator and the existing load/save helpers.
- Use the sibling-file `tests.rs` layout per CLAUDE.md.
- The flat bridge is still in the tree at this point; do not delete it
  here. Just make sure the new endpoints don't go through it — they
  read and write the structured `LibsecretSection` directly.
- Pair each endpoint with a one-line OpenAPI `#[utoipa::path]` block
  alongside its handler, matching the existing pattern in
  `admin_api/secrets.rs`.
