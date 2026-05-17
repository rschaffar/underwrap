# Task: Add backend admin CRUD for stores, instances, and mapping

Parent: `320_story_backend_managed_stores_and_runtime_instances.md`

## Scope

Expose backend admin/API support for managing the new backend-owned model:

- store CRUD
- client-instance CRUD/status operations as needed by the product design
- instance→store mapping mutation
- deletion protections for mapped stores

## Acceptance criteria

- [ ] Backend admin surface can manage stores.
- [ ] Backend admin surface can manage client-instance records and their store mapping.
- [ ] Store deletion is refused while an instance remains mapped to that store.
