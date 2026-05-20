# Epic: Atomic per-resource blobs with checkout/checkin and last-used

## Background

A resource today is decomposed into many backend keys — one per
environment variable, one per file path, and several per libsecret item
(`<col>/_meta`, `<col>/<item>.meta`, `<col>/<item>.secret`, `_aliases`).
Storage, IPC, and admin endpoints are all per-key. As a result:

- libsecret mutations write through to the backend one D-Bus call at a
  time (`crates/frontend-secretsd/src/dbus_service/collection.rs:120`,
  `item.rs:106`). Each call costs a daemon round trip and an HTTPS
  request to `underwrap-backendd`.
- The `files` slot loads per-key at namespace start and reconciles
  per-key after `waitpid` (`crates/frontend-ns/src/directory_credential.rs:237`).
  It is also bind-mounted **read-only** when the grant disallows
  writes, which breaks tools that try a test write before falling back
  (e.g. cache directories, git credential helpers).
- The two protocols are shaped differently for no good reason — one
  is per-call write-through, the other is load-then-batch-on-exit. A
  reader of the codebase can't predict either from the other.
- There is no persisted record of when a resource was last used by a
  given wrapper. `audit.rs` emits structured events but nothing
  persists them.

## Product decisions already made

- A resource is **one storage key**. The value is a JCS-canonical JSON
  blob (RFC 8785, via `serde_jcs`) holding env values, files tree, and
  libsecret keyring in one structure.
- Wrappers talk to underwrapd with three verbs, replacing every per-key
  libsecret/directory verb:
  - `Checkout { resource_id } → { blob, writable }`
  - `Checkin { resource_id, new_blob }`
  - `UsageSignal { resource_id }`
- The wrapper does the entire run against a local buffer (in-process
  for libsecret; tmpfs for files). At end of run it JCS-serializes the
  current state and byte-compares to the captured Checkout bytes:
  - unchanged → `UsageSignal`
  - changed + writable → `Checkin`
  - changed + not writable → log to journal, write to stderr unless
    `UNDERWRAP_SUPPRESS_READONLY_WARNING=1`, then `UsageSignal`
- The directory tmpfs is **always writable** inside the namespace. The
  server is the sole authority for write authorization (rejecting
  Checkin) and acts only as a safety net for the rare grant-flipped
  mid-run race.
- `WrapperResourcePairRecord` gains `last_used_at: Option<u64>`,
  bumped server-side on every Checkout (and as a side effect of every
  Checkin and UsageSignal). Surfaced in the admin matrix.
- Schema for the blob is integer / string / bool only — no floats,
  no `Option` ambiguity in a way that flips serialization shape, all
  maps and sets ordered. Round-trip stability is testable.
- Greenfield: no migration story. The on-disk layout for plain and
  encrypted stores changes shape; existing dev state is throwaway.

## Goals

1. One blob per resource in the `StorageBackend`. Per-key fan-out for
   env / files / libsecret is gone.
2. New IPC surface: `Checkout`, `Checkin`, `UsageSignal` replace the
   per-call libsecret and directory verbs in `shared-protocol`.
3. secretsd holds the resource blob in memory for the lifetime of the
   namespace and writes back only the diff (i.e. nothing if no change).
4. Directories mount read-write inside the namespace regardless of
   grant; server rejects writeback on read-only grants; wrapper warns.
5. `last_used_at` on every pair, updated on every wrapper run, shown
   in the admin matrix.
6. Documentation and the VM test reflect the new model.

## Stories

- `400_story_resource_blob_storage_and_protocol.md` — schema, storage
  layout, new IPC verbs, backend admin endpoints, foundation tests.
- `410_story_wrapper_buffered_writeback_and_warnings.md` — secretsd
  full-blob residency, writable tmpfs, end-of-run decision, journal /
  stderr warning with suppression env var.
- `420_story_last_used_on_pair_records.md` — pair record field, bump
  path, admin API surface, SPA matrix cell.
- `430_story_atomic_blob_documentation_and_vm_cutover.md` — docs,
  examples, CLAUDE.md, VM end-to-end test.

## Out of scope

- Optimistic concurrency (etag/version on Checkin). Last-write-wins
  is intentional.
- Partial / structural Checkin (e.g. "update just this env var on
  the server"). Admin edits go through the same load-edit-write loop.
- A new external-tool signing or verification flow for blobs.
  Canonical JSON keeps the door open but no consumer is planned in
  this epic.
- Hashed-blob delta protocol. Full blob on the wire each Checkin.
