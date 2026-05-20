# Epic: In-memory master keys for encrypted stores

## Background

The 32-byte master key for an encrypted store currently lives in a `0600`
file at `keys_dir/<store_id>.key` and is read by `underwrap-backendd` at
startup. That defeats the at-rest encryption promise: anyone with read
access to the backend filesystem (root, backup tape, leaked image,
accidental `scp`) lifts the key right next to the ciphertext it protects.
The encryption ends up adding bytes on disk without changing the actual
threat model.

This epic removes the on-disk key entirely. Master keys exist **only in
process memory**: generated once at store creation, displayed once to the
operator, kept by the backend for the lifetime of the process, zeroed on
shutdown or explicit lock.

Greenfield posture per CLAUDE.md applies: no migration path for existing
key files. Operators salvage their existing hex out-of-band (e.g.
`xxd -p < /var/lib/underwrap-backend/keys/<id>.key`) and paste it into the
new unlock dialog. Any code path that previously loaded `key_file` is
deleted, not preserved as a fallback.

## Product decisions

- **Unlock material** is the raw 64-char hex key. No passphrase + KDF.
- **No migration code.** Existing key files are abandoned; the operator's
  rescue path is "open the file, copy the hex, paste into Unlock".
- **Rotation** still mints a fresh random key and returns it once, but
  now re-encrypts every resource in the store under the new key (so a
  rotated store has no ciphertext under the old key when finished).
- **Rotation blocks writes** on the store while it runs. No live
  dual-key window.
- **Lock action** exists alongside Unlock so admins can re-lock without
  restarting the daemon.

## Goals

1. `StorageConfig::Encrypted` no longer carries `key_file`. The
   `keys_dir` `AdminRuntimeState` field and any matching CLI / NixOS
   knobs are gone.
2. `EncryptedBackend::open_with_key(dir, [u8; 32])` replaces
   `open(dir, key_file)`. All key-file parsing in `shared-store` is
   deleted.
3. `StoreRegistry` distinguishes `Locked` vs `Unlocked(Arc<dyn
   StorageBackend>)` per store. Plain stores are always `Unlocked`;
   encrypted stores boot `Locked` and require an explicit Unlock.
4. New protocol error `ErrorCode::StoreLocked`. Service handlers return
   it for any request that lands on a locked store.
5. New admin endpoints `POST /v1/admin/stores/{id}/unlock` and
   `/lock`. Unlock validates the hex via a canary file written at
   creation; wrong key → structured error, store stays locked.
6. `rotate-key` refuses on locked stores, takes a per-store write lock,
   re-encrypts every blob (including the canary), swaps the active
   backend.
7. Admin SPA surfaces lock state on the stores page with Unlock / Lock
   actions and an updated rotate dialog warning.
8. Docs (`docs/backend.md`, `docs/security.md`), the VM test, and the
   integration tests all reflect the new model.

## Stories

- `500_story_in_memory_master_keys.md` — the single rollout story for
  this epic, listing the task tickets below.

## Out of scope

- Sealing the in-memory key against `ptrace` / coredump beyond what
  `PR_SET_DUMPABLE=0` and `RLIMIT_CORE=0` already give us in the
  backend process.
- Hardware-backed key custody (TPM, YubiKey, KMS). The plain `[u8; 32]`
  in memory is the only custody model in this epic.
- Multi-operator quorum unlock (Shamir-style). Single-secret unlock.
- Passphrase support. Hex only.
- Auto-relock-after-N-seconds. The store stays unlocked until explicit
  lock or process exit.
