# Task: `rotate-key` re-encrypts all resources

Parent: `500_story_in_memory_master_keys.md`
Epic: `epic_in_memory_master_keys.md`

## Scope

- Rewrite `handle_admin_store_rotate_key` in
  `crates/backend-backendd/src/server.rs`:
  1. Look up the record; reject plain stores (existing behaviour).
  2. If the store is `Locked`, return 503 + `store_locked`.
     Operator must unlock first.
  3. Acquire a per-store rotation lock (see "Locking" below).
  4. Generate a fresh 32-byte key via `OsRng`.
  5. Construct a `next` backend via
     `EncryptedBackend::open_with_key` against the **same** data
     directory but in a staging subpath
     (`<dir>/.rotating-<random>/`). Write the canary into it first.
  6. Iterate `current.list("")` (excluding the canary — but the
     canary is filtered out at the registry layer already, so just
     iterate). For each id, `read` from `current`, `write` to
     `next`. If a read or write fails, abort: delete the staging
     directory, release the lock, return `Internal`.
  7. Atomically swap: rename `<dir>` → `<dir>.old-<random>`, rename
     staging → `<dir>`, delete `<dir>.old-<random>`. (Two renames
     on the same parent are atomic per-rename only — both must
     succeed for the rotation to be considered complete; if the
     second fails, attempt to roll the first back. Detail this in a
     small helper with a focused test.)
  8. Replace the registry entry's `Arc<dyn StorageBackend>` with a
     backend pointed at the new dir under the new key (re-`open_with_key`
     against the post-swap path). Drop the old `Arc` so its
     `MasterKey` zeroizes.
  9. Return `{ "key_hex": "...", "warning": "..." }`. Zero the hex
     buffer and the `[u8; 32]` stack copy.
- Locking: introduce a `tokio::sync::Mutex` keyed by `store_id`
  in `AdminRuntimeState` (a `Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>`
  with a lazy-create helper). Rotation holds the per-store lock for
  its entire duration. Read paths do **not** take this lock. Writes
  during rotation must also take this lock (`handle_service_write_prefixes`,
  `handle_service_checkin`, `handle_service_delete`) so a writer
  observing the pre-swap backend cannot append after the staging
  directory has been frozen. Reads continue against the live
  backend — pre-swap they see the old key (which still decrypts
  fine); post-swap they see the new one.
- Rollback semantics: on any failure between steps 4 and 8, the
  registry must still point at a working backend. Test this
  explicitly with a mid-iteration fault injection (`list` returns a
  bogus id that fails to `read`).
- Tests:
  - `rotate_key_reencrypts_all_resources`: seed N resources via the
    runtime API, rotate, confirm every resource still reads with the
    same plaintext.
  - `rotate_key_refuses_locked_store`: lock the store, attempt
    rotation, expect 503 / `store_locked`, no state change.
  - `rotate_key_rolls_back_on_failure`: inject a failure mid-rotation
    (e.g. seed a malformed file directly on disk), rotation aborts,
    follow-up read still returns the original plaintext under the
    original key.
  - `rotate_key_serialised_with_concurrent_write`: parallel rotate +
    write; only one progresses at a time and both complete with
    consistent ciphertext.
  - `rotate_key_plain_store_refused`: existing behaviour preserved.

## Acceptance criteria

- [ ] After a successful rotation, no file in the store decrypts
      with the old key.
- [ ] The canary is rotated alongside other resources.
- [ ] Concurrent writes during rotation either land in the new
      backend or are serialised behind it; nothing lands in the
      orphaned old data dir post-swap.
- [ ] Rollback on failure leaves the registry pointing at a working
      backend under the original key.
- [ ] `cargo test -p underwrap-backendd` green.

## Out of scope

- Streaming progress events to the SPA. Rotation is synchronous and
  returns 200 when done.
- Bounding rotation runtime / batch size. Stores are small enough
  in practice.
