# Task: Admin `unlock` / `lock` endpoints with canary validation

Parent: `500_story_in_memory_master_keys.md`
Epic: `epic_in_memory_master_keys.md`

## Scope

- New routes in `crates/backend-backendd/src/server.rs`:
  - `POST /v1/admin/stores/{id}/unlock`, body `{ "key_hex": "<64 hex>" }`,
    response `204 No Content` on success.
  - `POST /v1/admin/stores/{id}/lock`, no body, response `204 No
    Content`.
- Admin-only (existing `is_admin()` gate).
- Unlock handler (`handle_admin_store_unlock`):
  1. Look up store record. 404 if missing.
  2. Reject if the record is `StorageConfig::Plain` (400 +
     `code: "store_not_encrypted"`).
  3. Reject if the store is already `Unlocked` (409 + `code:
     "already_unlocked"`).
  4. Parse the hex via `underwrap_store::parse_hex_key`. 400 +
     `code: "invalid_key_hex"` on parse failure.
  5. Construct an `EncryptedBackend::open_with_key(dir, key)`.
  6. Read the canary at the well-known id (see "Canary" below).
     If the read fails (decryption error / canary missing), drop the
     backend, return 403 + `code: "key_mismatch"`. Do not transition
     the registry.
  7. On success, install the backend via `registry.unlock(...)`. Audit
     event `action=store_unlock, result=ok`. Zero the `[u8; 32]`
     stack copy as soon as it has been moved into the backend.
- Lock handler (`handle_admin_store_lock`):
  1. Look up store record. 404 if missing.
  2. Plain store: 400 + `code: "store_not_encrypted"`.
  3. Already locked: 200 / 204 idempotent. Audit
     `action=store_lock, result=noop`.
  4. Otherwise call `registry.lock(...)`, dropping the
     `Arc<dyn StorageBackend>`. The `MasterKey` zeroizes on drop.
     Audit `action=store_lock, result=ok`.
- Surfacing in admin store list / detail: extend the existing
  `StoreSummary` / `StoreDetail` view-model types with `locked: bool`.
  Regenerate the TS bindings (`ts-rs`).
- Canary: ticket 550 writes the canary on creation. For this ticket,
  treat the canary path as a constant in `shared-store`:
  `pub const ENCRYPTED_CANARY_ID: &str = "__underwrap__/canary";` and
  the canary plaintext as a fixed byte string
  `b"underwrap-encrypted-store-canary-v1"`. The unlock handler reads
  exactly this path and validates the plaintext matches before
  succeeding. Filter the canary from `list("")` results returned to
  any consumer (admin secret browser, runtime list) — implement the
  filter at the registry layer as a thin wrapper around the
  `StorageBackend` trait, applied **only to encrypted backends** in
  the `Unlocked` insertion path.
- Tests in `crates/backend-backendd`:
  - unlock with correct hex → 204; subsequent `read_prefixes` works.
  - unlock with wrong hex → 403, registry stays locked, follow-up
    request still returns `StoreLocked`.
  - unlock with malformed hex → 400, registry stays locked.
  - unlock plain store → 400.
  - unlock already-unlocked store → 409.
  - lock unlocked store → 204; follow-up request returns
    `StoreLocked`.
  - audit log entries emitted on unlock-ok, unlock-mismatch, lock-ok.

## Acceptance criteria

- [ ] Unlock and lock endpoints exist and are admin-gated.
- [ ] Wrong key never transitions the registry to `Unlocked`.
- [ ] Canary is hidden from `list("")`.
- [ ] Audit events match the existing `log_admin_event` shape.
- [ ] `cargo test -p underwrap-backendd` green.

## Out of scope

- Writing the canary (lives in 550 store-create + 560 rotate-key).
- SPA changes (570).
