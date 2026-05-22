# Task: Extract `fresh_id(prefix, exists)` helper

Parent: `1130_story_admin_api_refinement.md`
Resolves: F-006 (review report `admin_api_2026-05-22.md`)

## Background

Two id-generation loops are mechanically identical:

```rust
// stores.rs (handle_admin_store_create after 1140)
let store_id = loop {
    let candidate = generate_store_id();
    if !state.stores.read().unwrap().records.contains_key(&candidate) {
        break candidate;
    }
};

// client_instances.rs (handle_admin_client_instance_create after 1140)
let id = loop {
    let candidate = generate_client_instance_id();
    if !state.client_instances.read().unwrap().contains_key(&candidate) {
        break candidate;
    }
};
```

Both come from `crate::ids::random_id` underneath. The loop is a
paranoia guard against the ~80-bit collision space.

## Scope

In `crates/backend-backendd/src/ids.rs`:

- Add a small helper next to `random_id`:

  ```rust
  pub(crate) fn fresh_id<F: Fn(&str) -> bool>(prefix: &str, exists: F) -> String {
      loop {
          let candidate = random_id(prefix);
          if !exists(&candidate) {
              return candidate;
          }
      }
  }
  ```

In `crates/backend-backendd/src/admin_api/stores.rs` (post-1140 names):

- Replace the loop in `handle_admin_store_create` with one
  `crate::ids::fresh_id(...)` call. The closure does the `RwLock`
  read + `contains_key`.

In `crates/backend-backendd/src/admin_api/client_instances.rs`
(post-1140 names):

- Replace the loop in `handle_admin_client_instance_create` the same
  way.

`generate_store_id` and `generate_client_instance_id` become one-line
wrappers over `random_id` if they aren't already; leave them in place
so the prefix strings stay co-located with their callers (or inline the
prefix at the call site — judgement call, but don't proliferate
one-liners).

Resource id generation in `runtime_api/wrappers.rs::generate_resource_id`
does **not** retry on collision today. Leave it alone — that's a
separate question routed out of this epic.

## Out of scope

- Changing `random_id` or its entropy.
- Adding retry to resource-id generation.

## Acceptance criteria

- [ ] `crate::ids::fresh_id` exists and is used by both
      `handle_admin_store_create` and
      `handle_admin_client_instance_create`.
- [ ] No `loop { let candidate = generate_*_id(); ... }` block
      remains in either caller.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
