# Task: Resolve runtime store mapping from backend state

Parent: `320_story_backend_managed_stores_and_runtime_instances.md`

## Scope

Change runtime request handling so the backend resolves access from current backend state rather than static config:

- cert maps to current credential
- credential maps to active client instance
- instance maps to current store
- runtime service uses that store for the request

## Acceptance criteria

- [ ] Static `service.clients` mapping is no longer the runtime authorization source.
- [ ] Runtime requests resolve the current mapped store from backend state.
- [ ] Unknown/disabled instances or credentials fail runtime authorization.
