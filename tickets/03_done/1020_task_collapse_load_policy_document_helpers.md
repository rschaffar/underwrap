# Task: Collapse `load_policy_document` and `load_policy_document_or_default`

Parent: `1000_story_backend_backendd_refinement.md`
Resolves: F-005 (review report `backend-backendd_2026-05-21.md`)

## Background

`crates/backend-backendd/src/storage/documents.rs` has two helpers
that do the same thing:

```rust
pub(crate) fn load_policy_document(store: &dyn StorageBackend) -> Result<PolicyDocument> {
    match store.read(POLICY_DOCUMENT_SECRET_ID)? {
        Some(material) => serde_json::from_slice(material.as_ref())
            .context("parsing backend policy document"),
        None => Ok(PolicyDocument::default()),
    }
}

pub(crate) fn load_policy_document_or_default(
    store: &dyn StorageBackend,
) -> Result<PolicyDocument> {
    match store.read(POLICY_DOCUMENT_SECRET_ID)? {
        Some(material) => Ok(serde_json::from_slice(material.as_ref())?),
        None => Ok(PolicyDocument::default()),
    }
}
```

The only meaningful difference is the `.context(…)` annotation on the
parse error. Both default on `None`. Different call sites use
different variants for no semantic reason.

## Scope

In `crates/backend-backendd/src/storage/documents.rs`:

- Keep `load_policy_document` (the version with `.context(…)`).
- Delete `load_policy_document_or_default`.

Update every call site to use `load_policy_document`:

- `crates/backend-backendd/src/admin_api/policy.rs` — multiple uses
  inside the wrapper-permission helpers and `drop_client_instance_permissions`.
- `crates/backend-backendd/src/admin_api/resources.rs` —
  `handle_admin_resource_delete`.
- `crates/backend-backendd/src/admin_api/pairs.rs` —
  `handle_admin_pair_delete`.
- `crates/backend-backendd/src/admin_api/overview.rs` —
  `admin_ui_overview_for_store` (already uses the keeper, sanity check).
- `crates/backend-backendd/src/runtime_api/wrappers.rs` —
  `handle_runtime_wrapper_register`.
- `crates/backend-backendd/src/runtime_api/invocations.rs` — open
  and finalize handlers.
- `crates/backend-backendd/src/test_support.rs` — `RuntimeApiFixture::policy`.
- `crates/backend-backendd/src/storage/documents/tests.rs` — if it
  names the dropped helper.

While here, in `crates/backend-backendd/src/admin_api/policy.rs::handle_admin_policy_get`,
replace the inline `store.read(POLICY_DOCUMENT_SECRET_ID).and_then(...)`
reinvention with a call to `load_policy_document`.

## Out of scope

- Any change to the JSON shape of `PolicyDocument`.
- Touching the other `load_*_document` helpers
  (`load_resources_document`, `load_pairs_document`). Those don't
  have a duplicate sibling.

## Acceptance criteria

- [ ] `grep -rn "load_policy_document_or_default" crates/backend-backendd/`
      returns no matches.
- [ ] `policy.rs::handle_admin_policy_get` no longer reads
      `POLICY_DOCUMENT_SECRET_ID` inline.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
