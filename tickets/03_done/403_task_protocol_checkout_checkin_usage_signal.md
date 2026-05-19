# Task: Checkout / Checkin / UsageSignal IPC verbs

Parent: `400_story_resource_blob_storage_and_protocol.md`

## Scope

Replace the per-call libsecret and directory verbs on the wrapper-side
IPC channel with three bulk verbs. The channel itself stays a single
socketpair fd handed from underwrapd to the wrapper (currently fd 4
inside the namespace); it is now uniform for libsecret and files
(secretsd consumes the libsecret section, frontend-ns consumes the
files section).

### Protocol

```rust
pub enum WrapperResourceRequest {
    Checkout    { resource_id: String },
    Checkin     { resource_id: String, blob: Vec<u8> },  // JCS bytes
    UsageSignal { resource_id: String },
}

pub enum WrapperResourceResponse {
    CheckoutOk { blob: Vec<u8>, writable: bool },
    CheckinOk,
    UsageSignalOk,
    Error { code: ResourceErrorCode, message: String },
}

pub enum ResourceErrorCode {
    Unknown,
    Unauthorized,    // Checkin against a read-only grant
    NotFound,        // resource_id is not in scope of this wrapper run
    InvalidRequest,
    Internal,
}
```

### Removals

Delete:
- `SecretsdRequest::{Read,Write,List,Delete}` and the matching
  responses.
- `DirectoryRequest::{ListFiles,ReadFile,WriteFile,DeleteFile}` and
  the matching responses.
- The audit functions tied to these (`secretsd_read`, `secretsd_write`,
  `secretsd_delete`, directory equivalents). Replace with two events:
  `resource_checkout`, `resource_checkin` (with `accepted: bool`),
  and `resource_usage_signal`.

### Daemon handler

Replace `secretsd_channel.rs` and `directory_channel.rs` with one
`wrapper_resource_channel.rs` that:

- On `Checkout`: loads `resources/<resource_id>` via `store.read()`,
  decodes to a `ResourceBlob`, re-encodes via `to_canonical_bytes`,
  returns it along with the `writable` flag from the grant decision
  for this `(client_instance, wrapper, user, resource)`. Bumps
  `last_used_at` on the pair (the side that owns that storage is in
  task 421; for this task, leave a clearly-named TODO call site).
- On `Checkin`: validates `writable`; on read-only grant returns
  `Error::Unauthorized` (no storage write). On accept, decodes the
  incoming bytes to `ResourceBlob` (rejects schema-version mismatch),
  re-encodes to canonical bytes (rejects mismatch with the input —
  the wrapper must send already-canonical bytes), writes
  `resources/<resource_id>`. Atomic at the storage layer per task 402.
- On `UsageSignal`: scope-check only; bump `last_used_at`.

## Acceptance criteria

- [ ] New protocol types in `shared-protocol/src/message/` (or
      equivalent), under a clean module name (`wrapper_resource.rs`).
- [ ] Old libsecret/directory protocol enums and channels are
      deleted, not feature-flagged or shimmed.
- [ ] Daemon handler is a single file replacing the two old ones.
- [ ] Unit tests for the handler cover: `Checkout` returns blob +
      writable flag, with grant variations (locked, unlocked,
      writable=false, writable=true); `Checkin` accepted on writable
      grant and stored; `Checkin` rejected on read-only grant
      (no storage call); `Checkin` rejected on non-canonical input
      bytes; `UsageSignal` accepted; unknown `resource_id` rejected;
      grant-flipped-mid-request race produces `Unauthorized`.
- [ ] `cargo test --workspace` + clippy green.

## Out of scope

- The wrapper-side consumer rewrite (secretsd, frontend-ns) — story 410.
- `last_used_at` persistence — task 421 (this task just opens the
  call site).
- Admin endpoints — task 404.
