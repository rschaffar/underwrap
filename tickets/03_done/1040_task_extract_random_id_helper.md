# Task: Extract a shared random-id helper for store / client-instance / resource

Parent: `1000_story_backend_backendd_refinement.md`
Resolves: F-006 (review report `backend-backendd_2026-05-21.md`)

## Background

Three near-identical id generators live in three different modules:

- `crates/backend-backendd/src/store_registry.rs:283-291` —
  `generate_store_id` → `store_<20 hex chars>`.
- `crates/backend-backendd/src/admin_api/client_instances.rs:28-36` —
  `generate_client_instance_id` → `inst_<20 hex chars>`.
- `crates/backend-backendd/src/runtime_api/wrappers.rs:278-286` —
  `generate_resource_id` → `res_<20 hex chars>`.

All three call `rand::thread_rng().fill_bytes` on a 10-byte buffer
and hex-encode the result with the same `format!("{b:02x}")` map.
Wrappers.rs's rustdoc even calls out the relationship.

## Scope

Add a small module — pick one:

- a new `crates/backend-backendd/src/ids.rs` (preferred — short,
  obvious home), or
- a new `pub(crate) fn random_id(prefix: &str) -> String` next to
  `new_user_id` in `users.rs` if you'd rather not add a module.

Expose:

```rust
pub(crate) fn random_id(prefix: &str) -> String { … }
```

Plus, if ticket 1030 hasn't landed yet, a sibling helper for the
short hex suffix used by `stores.rs::stage_name`:

```rust
pub(crate) fn random_hex_8() -> String { … }
```

Rewrite the three callers as thin wrappers (or inline the call —
either way, the duplicate `format!("{b:02x}")` chains go).

If `users.rs::new_user_id` (16 random bytes hex-encoded, no prefix)
fits naturally on top of the same helper, refactor it to call
through too. Don't force it.

## Out of scope

- Changing the id length or prefix shape of any existing identifier.
  The wire format is stable; this is a pure-Rust factoring.

## Acceptance criteria

- [ ] `grep -rn "rand::thread_rng().fill_bytes" crates/backend-backendd/src/`
      returns at most one occurrence (the new helper), or zero if
      the helper uses a higher-level API.
- [ ] `generate_store_id`, `generate_client_instance_id`,
      `generate_resource_id` either delegate to `random_id(...)` or
      are inlined at their call sites.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
