# Task: Admin CRUD endpoints for Resources and Wrapper/Resource Pairs

Parent: `380_story_backend_resource_and_pair_entities.md`

## Scope

Add admin-only HTTP endpoints for Resources and Pairs:

- `POST /v1/admin/resources/list` → `Vec<ResourceRecord>` for a store
- `POST /v1/admin/resources/create` → creates with `created_by =
  <operator principal>`, refuses on duplicate name
- `POST /v1/admin/resources/delete` → refuses with structured response
  if any of the following depend on it:
  - any secret with id matching `<name>/(env|files|libsecret)/...`
  - any Pair with this resource
  - any grant with this resource
- `POST /v1/admin/pairs/list` → `Vec<WrapperResourcePairRecord>`
- `POST /v1/admin/pairs/create` → refuses if the named resource does
  not exist, refuses on duplicate `(wrapper, resource)`
- `POST /v1/admin/pairs/delete` → refuses if any grant references this
  pair

Delete-refusal responses follow the same shape used by store delete
(`AdminStoreDeleteResponse.cleanup_hint` / blocked-by counts), so the
SPA can render an operator-friendly list of what to clear first.

All endpoints require an authenticated admin operator; reuse the
existing CSRF and session middleware.

## Acceptance criteria

- [ ] Six endpoints wired into `server.rs` routing.
- [ ] Request and response types defined in `shared-protocol` (admin
      surface) and exported via ts-rs.
- [ ] Refusal responses identify the blocking dependents (counts and a
      sample of names) — enough information for the SPA to render a
      clear "fix this first" message.
- [ ] Unit tests cover the create + duplicate + delete-with-dependents
      cases for both entity types.

## Out of scope

- SPA wiring — covered under story 390.
