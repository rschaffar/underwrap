# Task: Stop reusing `validate_store_name` as a generic display-name validator

Parent: `1000_story_backend_backendd_refinement.md`
Resolves: F-010 (review report `backend-backendd_2026-05-21.md`)

## Background

`crates/backend-backendd/src/store_registry.rs:274-281` defines:

```rust
pub(crate) fn validate_store_name(name: &str) -> std::result::Result<String, &'static str> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        Err("store name must not be empty")
    } else {
        Ok(trimmed.to_string())
    }
}
```

It does "trim and reject empty" — nothing store-specific. But
`admin_api/client_instances.rs:193,329` calls it on a *display
name* and labels the error code `invalid_display_name`. The function
name suggests stronger validation than it performs, and the
client-instance call sites read confusingly.

## Scope

Pick one of:

**Option A — rename.** Move the helper to a new
`crates/backend-backendd/src/text.rs` (or fold into a generic
location like `crate::ids` if ticket 1040 created one) as:

```rust
pub(crate) fn validate_nonempty_display(value: &str) -> std::result::Result<String, &'static str> { … }
```

Update both `stores.rs` and `client_instances.rs` call sites.
Update the unit tests in `store_registry/tests.rs` if they name it.

**Option B — inline.** Inline the three-line check at the three
call sites in `stores.rs::do_admin_store_create`,
`stores.rs::do_admin_store_update`, and
`client_instances.rs::{do_admin_client_instance_create,
do_admin_client_instance_update}`. Delete the helper.

Either way, the misleading `validate_store_name` name disappears.

## Out of scope

- Adding stronger validation (length caps, character classes, etc.).
  The wire format hasn't asked for it, and a refactor pass is not
  the place to expand semantics.

## Acceptance criteria

- [ ] `grep -rn "validate_store_name" crates/backend-backendd/` returns no
      matches (or matches only the renamed helper if Option A).
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
