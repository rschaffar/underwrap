# Task: Collapse `_endpoint`/inner-handler pairs where no test calls the inner form

Parent: `1300_story_backend_backendd_refinement.md`
Resolves: F-006 (review report `backend_backendd_2026-05-22.md`)

## Background

Every admin module has at least one
`handle_admin_*_endpoint(state, ..., request)` /
`handle_admin_*(state, ..., &body)` pair, where the `_endpoint`
wrapper does `parse_json_body` then forwards to the inner function.
The split is justified when a unit test drives the inner form (we
can't synthesise a `Request<Incoming>`). It isn't justified when
the inner function has no direct test caller — which is the case
for roughly half the pairs.

Pair sites:
- `pairs.rs:40-79` (3 pairs).
- `policy.rs:121-167` (3 pairs).
- `resources.rs:40-126` (6 pairs).
- `client_instances.rs:173-183, 296-307` (2 pairs).
- `secrets.rs:101-115, 193-206` (2 pairs).
- `users.rs:117-127, 209-220` (2 pairs).
- `stores/crud.rs:90-100, 251-262` (2 pairs).
- `stores/lifecycle.rs:368-383` (1 pair).

The previous epic's ticket 1140 (`unify_body_handler_split_pattern`)
established the convention; this ticket is the
"now-actually-collapse-it-where-not-needed" follow-up.

## Scope

For each pair in the locations above:

1. Search the crate's `tests.rs` files for direct calls to the inner
   `handle_admin_*` form (the one taking `&body`).
2. If no test calls it: inline `parse_json_body` into a single
   `handle_admin_*` function that takes `request: Request<Incoming>`,
   and delete the `_endpoint` wrapper. Update the `route()` dispatcher
   to call the single function.
3. If a test does call it: keep the split, but rename so the HTTP
   entry-point is the unsuffixed `handle_admin_*` and the body-taker
   is the descriptive `handle_admin_*_with_body` (or rename the test
   to use a real bytes round-trip if that's easier).

Known test callers (keep the split for these — verify against the
current `tests.rs` files):
- `handle_admin_store_create` — called by
  `crates/backend-backendd/src/test_support.rs:create_store_for_test`.
- `handle_admin_store_update` — used by several `stores/crud/tests.rs`
  tests.

Likely-collapsible candidates (verify the test surface first):
- `pairs.rs` — list, create, delete pairs.
- `policy.rs` — wrapper-permissions list, upsert, delete.
- `resources.rs` — list, create, delete, rename, blob/get, blob/put.
- `client_instances.rs` — create, update.
- `secrets.rs` — write, rename.
- `users.rs` — create, update.

### After the mechanical pass

Re-read each touched admin module against the simplification checklist
(category 7a in the skill) and remove anything still fitting the
bullet list: single-caller wrappers, defensive branches that can't
fire, redundant clones, etc.

## Acceptance criteria

- [ ] Every `_endpoint` function that survives the audit has at
      least one direct test caller for the inner form, documented
      in a comment at the wrapper.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
