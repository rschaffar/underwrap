# Task: Fix `_aliases` re-emission so admin delete is observable

## Background

`crates/shared-store/src/resource_blob.rs::libsecret_section_to_kv`
currently emits a synthetic `_aliases` flat key whenever **either** the
alias map is non-empty **or** any collection exists:

```rust
if !section.aliases.is_empty() || !section.collections.is_empty() {
    let aliases_json = serde_json::to_vec(&section.aliases)?;
    out.insert("_aliases".to_string(), SecretBytes::from(aliases_json));
}
```

That second clause is the bug. With it in place, deleting `_aliases`
through the admin API silently appears to do nothing:

1. `delete_resource_entry` expands the blob, removes the `_aliases`
   entry, recomposes the blob. The persisted alias map is now empty.
2. The audit event correctly reports `secret.delete result=success`.
3. The next list / read call goes back through
   `libsecret_section_to_kv`. Because the resource still has
   collections, the function re-creates `_aliases` as
   `serde_json::to_vec(&empty_map)` → `{}`.
4. The operator sees `_aliases` reappear in the admin UI.

The `|| !section.collections.is_empty()` clause has no downstream
consumer — `grep '"_aliases"' --include=*.rs` only matches this file —
so dropping it is safe.

This is a stop-gap fix to unblock the operator workflow while the
larger structured-admin-surface rework (see
`epic_libsecret_structured_admin.md`) lands.

## Goal

Make admin delete of `_aliases` observable: after a successful delete,
the entry stays gone until an actual alias is set again.

## Scope

### 1. Tighten the emission condition

In `crates/shared-store/src/resource_blob.rs::libsecret_section_to_kv`,
change

```rust
if !section.aliases.is_empty() || !section.collections.is_empty() {
```

to

```rust
if !section.aliases.is_empty() {
```

`libsecret_section_from_kv` already handles a missing `_aliases` by
falling back to an empty `BTreeMap`, so the inverse direction stays
sound.

### 2. Cover the regression in tests

In `crates/shared-store/src/resource_blob/tests.rs`, add at least:

1. **Empty aliases with collections → no `_aliases` row.**
   - Build a `LibsecretSection` with one collection containing one item,
     and `aliases` empty.
   - Assert `expand_blob_to_entries` output does not contain
     `<rid>/libsecret/_aliases`.
2. **Non-empty aliases → `_aliases` row present and round-trips.**
   - Build a section with `aliases = {"default": "default"}` and one
     collection.
   - Assert `expand_blob_to_entries` contains the `_aliases` row with
     the expected JSON value.
   - Round-trip through `blob_from_resource_entries` and assert the
     aliases map survives.
3. **Delete-aliases idempotency.**
   - Expand a section, drop the `_aliases` entry, recompose, expand
     again, assert no `_aliases` row.

## Out of scope

- The structured admin-API rework (separate story).
- Any data-model change.
- Removing the flat bridge.
- SPA changes.

## Acceptance criteria

- [ ] `libsecret_section_to_kv` no longer emits `_aliases` when the
      alias map is empty, even if collections exist.
- [ ] New tests in `resource_blob/tests.rs` cover the three cases above
      and pass.
- [ ] `cargo test -p underwrap-store` passes.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] Manual repro from the original bug report: delete `_aliases` via
      the admin UI, refresh the resource page, entry stays gone.

## Notes for the implementer

- This is a one-line code change plus tests. Resist the urge to also
  retire the flat bridge here — that belongs in
  `2310_task_strip_libsecret_from_flat_bridge.md` so it can be reviewed
  alongside the structured-endpoint work.
- The audit event shape stays unchanged for now; the structured audit
  events come with the structured endpoints.
