# Task: De-duplicate `mapped_client_instance_count`

Parent: `1130_story_admin_api_refinement.md`
Resolves: F-007 (review report `admin_api_2026-05-22.md`)

## Background

`admin_store_usage` and `admin_store_usage_for_locked` in
`crates/backend-backendd/src/admin_api/stores.rs` (or `stores/mod.rs`
after 1200) each carry the same projection:

```rust
saturating_u32(
    state.client_instances.read().unwrap().values()
        .filter(|instance| instance.current_store_id == store_id)
        .count(),
)
```

Two lock acquires + filter, same shape. The two functions differ only
in whether they compute `has_secrets` (the unlocked path calls
`store.list("")`).

## Scope

In the same file (post-1200, `crates/backend-backendd/src/admin_api/stores/mod.rs`):

- Extract:

  ```rust
  fn mapped_client_instance_count(
      state: &AdminRuntimeState,
      store_id: &str,
  ) -> u32 {
      saturating_u32(
          state.client_instances.read().unwrap().values()
              .filter(|i| i.current_store_id == store_id)
              .count(),
      )
  }
  ```

- Replace both inline copies with a `mapped_client_instance_count(state, store_id)`
  call.

If this ticket lands before 1200, do it in the still-flat `stores.rs`
and 1200 will move the helper to `stores/mod.rs` along with the rest
of the shared projector code.

## Out of scope

- Touching `has_secrets` or any other field of the usage projections.
- Folding the two usage functions into one — they still legitimately
  differ on the `has_secrets` axis.

## Acceptance criteria

- [ ] One `mapped_client_instance_count` function exists; both callers
      use it.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
