# Task: Per-instance auto-create opt-in flag

Parent: `380_story_backend_resource_and_pair_entities.md`

## Scope

Add an `auto_create_wrapper_assets: bool` field to the client-instance
record (`crates/backend-backendd/src/...client_instances...`),
defaulting to `false`. The runtime register endpoint
(`382_task_replace_catalog_with_runtime_register_endpoint.md`) reads
this flag to decide whether missing entities may be created.

Admin surface: extend the existing
`AdminClientInstanceUpdateRequest` with an optional
`auto_create_wrapper_assets` field. Include the current value in the
client-instance detail view models so the SPA can render and toggle
it. No new endpoint needed.

## Acceptance criteria

- [ ] Field added to the persisted client-instance record with default
      `false` for existing rows on load.
- [ ] Updateable through the existing admin update endpoint.
- [ ] Exposed on the client-instance detail/list view models.
- [ ] Unit tests cover: default-false on a freshly-created instance,
      toggling via admin update, persistence across reload.

## Out of scope

- The SPA toggle UI (covered by
  `394_task_spa_per_instance_auto_create_toggle.md`).
