# Task: Add Resource and WrapperResourcePair data model

Parent: `380_story_backend_resource_and_pair_entities.md`

## Scope

Introduce two persisted record types in the backend, one collection
per store:

- `ResourceRecord { name, created_by: Principal, created_at: u64 }`
- `WrapperResourcePairRecord { wrapper, resource, created_by, created_at }`

Both `created_*` fields are write-once on create. `Principal` reuses
the existing operator/client-instance enum from
`crates/backend-backendd/src/operators.rs`.

Storage shape mirrors the existing per-store JSON document pattern
(see `__underwrap/policy/document` and the soon-to-be-removed
`__underwrap/catalog/document`): two new reserved secret IDs,
`__underwrap/resources/document` and `__underwrap/pairs/document`,
each holding a serialized `Vec<…>` plus a schema version tag for
forward-safety. The `RESERVED_PREFIX` filter in the SPA already hides
`__underwrap/` from operators (see `web/src/pages/secrets.tsx:179`),
so the new docs are invisible to the secret browser by construction.

Lookup helpers on the in-memory documents:

- `find_resource(name) -> Option<&ResourceRecord>`
- `find_pair(wrapper, resource) -> Option<&WrapperResourcePairRecord>`
- `pairs_for_resource(name) -> impl Iterator`

## Acceptance criteria

- [ ] Record types live in `shared-protocol` (wire shape exported via
      ts-rs so the SPA gets generated TS types) and the per-store
      document wrappers live in `backend-backendd`.
- [ ] Load + store helpers exist alongside the existing
      `load_policy_document` / `store_policy_document` pair, with the
      same "missing doc means empty collection" semantics.
- [ ] Schema version field present; deserialization rejects unknown
      versions with a clear error.
- [ ] Unit tests cover: empty load, round-trip, version-rejection,
      duplicate-name refusal on insert.

## Out of scope

- HTTP endpoints, runtime register, admin CRUD — those land in later
  tasks under this story.
