# Task: Use `save_policy_document` in `handle_admin_policy_put`

Parent: `1130_story_admin_api_refinement.md`
Resolves: F-005 (review report `admin_api_2026-05-22.md`)

## Background

Every policy mutator in `policy.rs` —
`drop_client_instance_permissions`,
`handle_admin_wrapper_permission_upsert`,
`handle_admin_wrapper_permission_delete` — persists through
`save_policy_document(store.as_ref(), &doc)` from
`crate::storage::documents`. Only `handle_admin_policy_put` reaches
past that helper:

```rust
let write_result = serde_json::to_vec(&body.document)
    .map_err(anyhow::Error::from)
    .and_then(|bytes| store.write(POLICY_DOCUMENT_SECRET_ID, bytes.as_slice()));
```

Ticket 1020 collapsed the `load_policy_document*` helpers but the
write side was missed. If `save_policy_document` ever adds
canonicalisation, validation, or audit hooks, this code path silently
skips them.

## Scope

In `crates/backend-backendd/src/admin_api/policy.rs`:

- Replace the inline `serde_json::to_vec` + `store.write` block in
  `handle_admin_policy_put` (lines ~100-125) with
  `save_policy_document(store.as_ref(), &body.document)`.
- The error mapping stays the same: on failure, return
  `error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())`
  (matching the existing pattern in the rest of the file).
- Drop the `POLICY_DOCUMENT_SECRET_ID` import if no other caller in
  the file uses it after this change.

## Out of scope

- Any change to `save_policy_document` itself.
- Renames or signature changes elsewhere.

## Acceptance criteria

- [ ] `rg -n 'POLICY_DOCUMENT_SECRET_ID' crates/backend-backendd/src/admin_api/policy.rs`
      returns no matches if the import was dropped (only matches if
      another helper in the same file still needs it).
- [ ] `handle_admin_policy_put` no longer calls `serde_json::to_vec`
      or `store.write` directly.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
