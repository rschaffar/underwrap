# Task: Fix the post-commit cascade in client-instance update

Parent: `1130_story_admin_api_refinement.md`
Resolves: F-010 (review report `admin_api_2026-05-22.md`)

## Background

In `client_instances::do_admin_client_instance_update` (renamed to
`handle_admin_client_instance_update` by 1140), when the operator
remaps an instance to a different store the in-memory map and the
on-disk file are committed *first*; only after that does the
`drop_client_instance_permissions` cascade on the **old** store's
policy document run. Today, if the cascade fails the caller sees:

```rust
return error_response(
    StatusCode::INTERNAL_SERVER_ERROR,
    "internal",
    &format!("clearing prior-store permissions: {e:#}"),
);
```

The update has already persisted — returning `500 internal` doesn't
undo the commit. Worse, a retrying client may assume the update didn't
take and resend. The old store's policy document keeps the now-stale
permissions for this instance, which is exactly what
`drop_client_instance_permissions` exists to prevent on a remap.

The delete path (`handle_admin_client_instance_delete`) already faced
this and chose log-and-continue, with an explicit comment: leaving
orphaned policy rows is preferable to failing the operation and
leaving the operator with no way to remove the instance at all.

User decision 2026-05-22: match the delete-path precedent. Log the
cascade failure with `tracing::warn!` carrying `client_instance`,
`store`, and the error, and return `200 OK` with the update applied.

## Scope

In `crates/backend-backendd/src/admin_api/client_instances.rs`:

- In `handle_admin_client_instance_update` (post-1140 name), replace
  the `return error_response(...)` arm on `drop_client_instance_permissions`
  failure with a `tracing::warn!` matching the delete-path shape:
  `client_instance = %id, store = %old_store_id, err = %e,
  "clearing prior-store permissions"`. Continue with the success
  response.
- If the existing delete-path code carries a comment explaining the
  log-and-continue choice, mirror that single-sentence justification
  here.

Add one unit test that drives the failure path:

- Set up state where the old store exists and has policy permissions
  for the instance, then arrange for `drop_client_instance_permissions`
  to return `Err(...)`. The cleanest hook is making the prior store
  locked (the helper's `unlocked_backend` lookup returns `Err` and the
  helper short-circuits to `Ok(())` — that already exercises the
  "skip when locked" arm); for an actual `Err` return, an alternative
  is to mock the store's `write` to fail, or to drive a corrupted
  policy document so `load_policy_document` errors.
- Assert: (a) the update is observable (next read returns the new
  `current_store_id`); (b) the response status is 200; (c) a
  warn-level log line is emitted (use `tracing-test` or the existing
  test infra if any). If neither test infra captures `warn!`, assert
  (a) and (b) only and add a comment noting the missing assertion.

## Out of scope

- Wire-shape change. No new `partial_cascade_warning` field, no
  response-body change. The error becomes a server-side journal
  event, not a client-visible signal.
- Touching the delete path itself.
- Any rename — 1140 already did them.

## Acceptance criteria

- [ ] `handle_admin_client_instance_update` returns 200 on cascade
      failure with a `warn!` log line.
- [ ] A unit test exercises the failure path and asserts the post-
      update state is observable.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
