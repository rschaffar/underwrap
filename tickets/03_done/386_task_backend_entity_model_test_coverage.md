# Task: Backend test coverage for Resource and Pair model

Parent: `380_story_backend_resource_and_pair_entities.md`

## Scope

Add or rewrite tests covering the new entity surface end-to-end on the
backend side:

- Unit tests: covered inside the data-model and endpoint tasks; this
  task is the cross-cutting glue.
- Integration tests in `crates/integration-tests/src/tests/`:
  - Replace `service_mode_registers_catalog_entries_on_startup` with a
    `service_mode_auto_creates_wrapper_assets_when_enabled` test.
  - Add a sibling `service_mode_refuses_auto_create_when_disabled`
    test asserting the `AutoCreateDisabled` response and provenance
    untouched.
  - Add a test that exercises admin create + delete-refusal flow:
    create a resource, write a secret under it, attempt delete →
    structured refusal; remove the secret → delete succeeds.
- VM test `nix/tests/test.py`: extend with an assertion that an
  opted-in instance, after first request, has a Resource and a Pair
  record persisted with `created_by` matching the instance's id.

## Acceptance criteria

- [ ] `cargo test -p integration-tests` green.
- [ ] VM test (`nix build .#checks.x86_64-linux.vm-test`) green.
- [ ] Each delete-refusal branch (secret blocker, pair blocker, grant
      blocker) has at least one assertion.

## Out of scope

- SPA Vitest and Playwright tests — covered by
  `395_task_spa_and_e2e_test_coverage_for_entity_model.md`.
