# Task: Add the runtime client-instance and current-credential model

Parent: `320_story_backend_managed_stores_and_runtime_instances.md`

## Scope

Introduce backend-managed runtime client-instance records:

- stable internal client-instance ID
- display name
- hostname hint
- attested configured instance name
- status / last-seen metadata
- exactly one current credential pointer/record

## Acceptance criteria

- [ ] Backend persists runtime client-instance records.
- [ ] Each instance has exactly one current credential in the model.
- [ ] Instance metadata supports display name, hostname hint, attested instance name, status, and last seen.
