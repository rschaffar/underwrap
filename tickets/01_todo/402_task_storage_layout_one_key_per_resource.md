# Task: One storage key per resource

Parent: `400_story_resource_blob_storage_and_protocol.md`

## Scope

Collapse the per-slot / per-item key fan-out into a single
`StorageBackend` key per resource holding the canonical
`ResourceBlob` bytes.

### Layout

- New key shape: `resources/<resource_id>` (no slot suffix).
- Old keys (`<resource>/env/<VAR>`, `<resource>/files/<path>`,
  `<resource>/libsecret/<col>/_meta`, `.meta`, `.secret`, `_aliases`)
  are gone — no readers, no writers. Update
  `crates/shared-protocol/src/resource.rs` to drop the prefix helpers
  that no longer make sense.

### Backend impls

- `PlainBackend`: one file per resource, written atomically via the
  existing `tempfile::persist` path. No directory-per-resource.
- `EncryptedBackend`: one nonce per resource blob (the value is the
  whole canonical-JSON bytes), AEAD as today. Note: the per-key crypto
  surface is now strictly smaller.
- `ServiceBackend` (mTLS HTTPS client): the per-key HTTP endpoints
  stay (read/write/delete/list/read-prefixes), but the keys passed
  through them are now `resources/<resource_id>` — the trait shape
  itself does not change, only the keys flowing through it.

### Reserved prefix

Existing reserved keys (`__underwrap/policy/document`,
`__underwrap/resources/document`, `__underwrap/pairs/document`,
`__underwrap/operators/document`, etc.) remain untouched.

### Helpers

Add to `shared-protocol`:

```rust
pub fn resource_blob_key(resource_id: &str) -> String {
    format!("resources/{resource_id}")
}
```

Use this consistently from underwrapd and backend-backendd.

## Acceptance criteria

- [ ] No code path reads or writes the old per-slot keys.
- [ ] One file per resource exists on disk for `PlainBackend` after a
      round trip; encrypted store equivalent.
- [ ] `list` of the resources prefix returns one entry per resource.
- [ ] `read_prefixes` for `resources/` returns full blob bytes for each
      resource and is the primary bulk-load path used by the daemon.
- [ ] Unit tests for both backend impls cover write → read round trip
      with a non-trivial blob; overwrite is atomic from the reader's
      POV (verified via an interleaved test or by inspecting the
      `persist` path).
- [ ] `cargo test --workspace` + clippy green.

## Out of scope

- Wire IPC verbs — task 403.
- Admin endpoints — task 404.
- Wrapper-side use of the new keys — story 410.
