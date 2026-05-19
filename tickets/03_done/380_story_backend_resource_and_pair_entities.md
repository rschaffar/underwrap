# Story: Backend Resource and Wrapper/Resource Pair entities

Parent: `epic_resources_and_pairs_as_entities.md`

## Outcome

After this story, every store has two new persisted collections —
Resources and Wrapper/Resource Pairs — each with provenance. Wrappers
auto-populate them on startup but only for client instances explicitly
opted in by an admin. The legacy catalog document and endpoints are
gone. Admin CRUD endpoints expose the new entities, with deletes that
refuse to break dependents.

## Tasks

- `381_task_backend_resource_and_pair_data_model.md`
- `382_task_replace_catalog_with_runtime_register_endpoint.md`
- `383_task_per_instance_auto_create_optin_flag.md`
- `384_task_admin_crud_endpoints_for_resources_and_pairs.md`
- `385_task_remove_catalog_document_and_endpoints.md`
- `386_task_backend_entity_model_test_coverage.md`

## Acceptance criteria

- [ ] Resources and Pairs persist with immutable `created_by` /
      `created_at` set on first creation.
- [ ] Runtime register endpoint upserts missing entities only when the
      calling instance is opted in; otherwise it returns a typed
      refusal.
- [ ] Admin endpoints can list, create, and delete both entity types.
- [ ] Delete refuses with a structured response when dependents exist
      (secrets under the resource, pairs referencing the resource,
      grants on the pair or resource).
- [ ] No code path reads or writes `__underwrap/catalog/document`.
- [ ] `cargo test --workspace` green.
