# Task: Drop `key_file` from `StorageConfig::Encrypted`; key is in-memory

Parent: `500_story_in_memory_master_keys.md`
Epic: `epic_in_memory_master_keys.md`

## Scope

- Remove the `key_file: PathBuf` field from
  `StorageConfig::Encrypted` in
  `crates/shared-protocol/src/config.rs`. The variant keeps `dir` and
  nothing else. Wire / JSON shape becomes `{"backend":"encrypted",
  "dir":"..."}`.
- `crates/shared-store/src/encrypted.rs`:
  - Delete `load_master_key` and the key-file permission check.
  - Replace `EncryptedBackend::open(dir, key_file)` with
    `EncryptedBackend::open_with_key(dir: &Path, key: [u8; 32]) -> Result<Self>`.
  - `MasterKey` stays as-is (`Zeroize + ZeroizeOnDrop`). The constructor
    moves the caller's `[u8; 32]` into the struct, then the caller's
    copy is dropped on the stack (use `MasterKey(key)` where `key` is
    consumed).
  - Re-export `MasterKey` is not needed; keep it crate-private.
  - Add `pub fn parse_hex_key(s: &str) -> Result<[u8; 32]>` in the
    crate root (`shared-store/src/lib.rs`) for shared use by unlock /
    rotate-key. Reject anything other than exactly 64 hex chars
    (trimmed). Constant-time comparison is not required.
- `crates/shared-store/src/lib.rs`: drop the `key_file` branch from
  the public `open()` dispatch — encrypted backends are no longer
  constructed by the generic `open()`. Callers that need to open an
  encrypted store must use `EncryptedBackend::open_with_key` directly
  (the registry code in `backend-backendd` does this).
- Loader of `stores.json` in `backend-backendd`: if a legacy record
  still carries `key_file`, `serde` deny-unknown-fields will reject
  the load. Add a one-time `tracing::warn!` and stripper if that
  ergonomic matters for in-tree dev environments; otherwise let the
  load fail loudly so operators know they need to clean up.
- Tests in `encrypted.rs`: rewrite the test helper to construct the
  backend via `open_with_key([u8; 32])`. Delete `hex_key_file`,
  `key_file mode` assertions, and any test that writes a key file.

## Acceptance criteria

- [ ] `StorageConfig::Encrypted` has only `dir`.
- [ ] `EncryptedBackend::open_with_key` is the only constructor; the
      old `open(dir, key_file)` is gone.
- [ ] `parse_hex_key` lives in `shared-store` and is unit-tested for
      length, casing, non-hex characters, and surrounding whitespace.
- [ ] `cargo test -p underwrap-store` green.
- [ ] `cargo test --workspace` builds; backend tests are updated in
      later tickets to actually exercise the new constructor.

## Out of scope

- The `StoreRegistry` lock-state model (510 → 520).
- The new unlock / lock admin endpoints (540).
- Any SPA change.
