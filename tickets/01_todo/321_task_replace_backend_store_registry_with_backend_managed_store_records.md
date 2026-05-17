# Task: Replace the backend store registry with backend-managed store records

Parent: `320_story_backend_managed_stores_and_runtime_instances.md`

## Scope

Replace backend-side declarative store management with backend-managed store records:

- define the persistent store record shape
- support stable internal IDs plus editable visible names
- keep `plain` and `encrypted` backend kinds
- remove the active runtime design's dependency on server-side declarative store definitions

## Acceptance criteria

- [ ] Backend persists store records with immutable IDs and editable names.
- [ ] `plain` and `encrypted` store kinds remain supported.
- [ ] Active runtime design no longer depends on backend-side declarative store definitions.
