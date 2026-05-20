# Task: `create-store` keeps the key in memory, writes the canary

Parent: `500_story_in_memory_master_keys.md`
Epic: `epic_in_memory_master_keys.md`

## Scope

- Rewrite `handle_admin_store_create` in
  `crates/backend-backendd/src/server.rs`:
  - For encrypted stores:
    1. Mint 32 random bytes via `rand::rngs::OsRng`.
    2. Build the `EncryptedBackend` via
       `EncryptedBackend::open_with_key(dir, key)`.
    3. Write the canary file
       (`ENCRYPTED_CANARY_ID`, fixed plaintext) to the backend.
       Failure here is fatal — clean up the data dir and return
       `Internal`.
    4. Install the backend in the registry as `Unlocked`. Persist
       the `StoreRecord` (without `key_file`) to `stores.json`.
    5. Return `{ "key_hex": "<hex>", "warning": "..." }` exactly
       once. After serialising the response, zero the hex string
       buffer and drop the `[u8; 32]` stack copy.
  - For plain stores: behaviour unchanged, no canary.
- Delete:
  - `generate_master_key_atomic` and any helper that wrote
    `keys_dir/<id>.key`.
  - The `keys_dir` field from `AdminRuntimeState` (already removed in
    520; re-check).
  - The `--keys-dir` CLI flag / `BackendConfig::keys_dir` /
    NixOS-module `keysDir` option.
- Update the admin store-create response type
  (`AdminStoreCreateResponse` / `AdminStoreRotateKeyResponse` reuse
  the same shape today — keep that). Regenerate TS bindings.
- Tests:
  - `test_admin_create_encrypted_store`: succeeds, returns hex,
    leaves no file under any keys directory, canary is present in
    the store directory but hidden from `list("")`.
  - `test_admin_create_encrypted_store_zeroizes_key_in_response`:
    optional — a hex-buffer pointer probe is brittle, skip if it
    requires unsafe.
  - `test_admin_create_plain_store_unchanged`: plain stores still
    work and do not write a canary.

## Acceptance criteria

- [ ] No code path writes a master key to disk.
- [ ] `keys_dir` plumbing is gone end-to-end (CLI, config struct,
      NixOS module, docs).
- [ ] Canary written on create and hidden from list.
- [ ] `cargo test -p underwrap-backendd` green.

## Out of scope

- Rotation re-encryption (560).
- Restart-survival of created stores (they become `Locked` on the
  next boot — that's correct, no code change needed beyond 520).
