# Story: Backend-managed stores and runtime client instances

Parent: `epic_dynamic_runtime_clients_enrollment_and_stores.md`

## Background

The current backend service model relies on declarative stores and static service-client config. We want the backend to own both concepts directly.

## Desired design

- Remove backend-side declarative store management.
- Replace the static runtime-client config model with backend-managed runtime client-instance records.
- Introduce backend-managed stores with:
  - stable internal `store_id`
  - editable visible name
  - optional description
  - existing storage backend kinds (`plain`, `encrypted`)
- Introduce backend-managed client instances with:
  - stable internal `client_instance_id`
  - editable display name
  - hostname hint
  - attested configured instance name
  - exactly one current mapped store
  - status / last seen
- Runtime request authorization resolves current store mapping from backend state, not static config.

## Tasks

1. `321_task_replace_backend_store_registry_with_backend_managed_store_records.md`
2. `322_task_add_runtime_client_instance_and_current_credential_model.md`
3. `323_task_resolve_runtime_store_mapping_from_backend_state.md`
4. `324_task_add_backend_admin_crud_for_stores_instances_and_mapping.md`
5. `325_task_backend_managed_store_and_instance_test_coverage.md`

## Acceptance criteria

- [ ] Declarative backend-store management is removed from the active runtime design.
- [ ] Static `service.clients` mapping is no longer the source of runtime authorization.
- [ ] Backend can persist and manage stores with immutable IDs and editable names.
- [ ] Backend can persist and manage runtime client-instance records with exactly one current store mapping.
- [ ] Store deletion is refused while an instance is mapped to that store.
- [ ] Runtime request handling can resolve the current mapped store from backend state.
