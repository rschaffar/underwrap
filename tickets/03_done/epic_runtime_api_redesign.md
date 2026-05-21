# Epic: Resource-vocabulary runtime API; server-side authorization; drop SSH approval

## Background

The runtime HTTPS API between `underwrapd` and `underwrap-backendd`
still speaks **storage vocabulary** — `ServiceWriteRequest` carries an
opaque `secret_id: String` that the client picks. The endpoint set is a
1:1 mirror of the `StorageBackend` trait (`/v1/read`, `/v1/write`,
`/v1/list`, `/v1/read-prefixes`, `/v1/delete`) and was designed back
when the runtime client *was* the storage backend.

Layered on top of that, two further patterns leak server-only
concerns into the client:

- **Policy evaluation runs in the daemon.** The daemon reads the
  `PolicyDocument` blob on every wrapper request and walks its rows by
  `(client_instance_id, wrapper, resource_id)`. The backend
  duplicates the gate, but the daemon still has to hold the policy
  schema, know its own principal id, and know backend resource ids.
- **Per-call SSH-agent approval verification runs in the daemon.**
  `crates/frontend-underwrapd/src/approval.rs` picks the nonce, holds
  the pending challenge map, and verifies the SSH signature against
  the configured `approval_key`. The backend has no equivalent check
  — a buggy or replaced daemon can just stop verifying.

The combination produces three classes of bug we are currently
working around:

1. **Storage-vocabulary leaks.** A runtime mTLS client can POST to
   `/v1/write` with `secret_id: "__underwrap/policy/document"` and a
   hand-crafted policy doc that grants the caller everything. The gate
   today passes `__underwrap/*` unconditionally because the daemon
   legitimately *reads* the policy doc — the same hole admits writes.
2. **Trust-in-client.** Approval verification, principal-id
   self-identification, and the `writable` flag are all things the
   daemon could lie about today; we just trust it not to. (We pinned
   the `writable` story with a daemon-bypass test, but principal and
   approval are still unprotected.)
3. **Stale layering.** The legacy `<resource>/<slot>/<inner>` per-key
   write shape from the pre-atomic-blob world is still on the wire
   (`runtime_read_gate` accepts it). Nothing modern uses it; it just
   sits there as an extra attack surface.

This epic deletes the legacy runtime API and replaces it with a
narrow, resource-vocabulary surface where the client never names a
storage key, never identifies itself by id, never evaluates policy,
and never verifies a signature. The backend is the sole authority.
Per-call SSH approval is removed entirely — operators who want a
per-invocation prompt should use OS-level mechanisms (sudo/polkit) for
that policy.

## Product decisions already made

- **Three runtime endpoints**, addressed by `wrapper_name` and
  `resource_name` only:
  - `POST /v1/wrappers` — register the wrapper set.
  - `POST /v1/wrapper-invocations` — open: returns the resource blob
    plus a `writable` hint, or Deny.
  - `POST /v1/wrapper-invocations/finalize` — close: with optional
    new blob; the backend validates and writes if writable; bumps
    `last_used_at` either way.
- **No sessions.** Open and finalize are independent stateless calls.
  Every call re-derives the principal from the mTLS cert and
  re-evaluates the policy from scratch. No server-side session map,
  no tokens, no TTLs.
- **No SSH approval.** `WrapperApprovalMode::PerCallApproval`,
  `WrapperRequest::ApprovalProof`, `WrapperResponse::ApprovalRequired`,
  `crates/frontend-underwrapd/src/approval.rs`, the
  `wrapper_flow::approval_round` module, and the SPA's per-call
  approval UI are deleted. `ssh-key` / `signature` /
  `ed25519-dalek` come off the daemon's dependency list.
- **No mount paths on the wire.** `WrapperResourcePairRecord` loses
  `files_mount_paths`; the register call doesn't carry them. Mount
  paths are a daemon-local fact (used by `frontend-ns` to set up bind
  mounts) and stay that way. The SPA's Secrets browser shows the file
  section's existing path-shaped keys directly.
- **Env section is read-only on writeback.** A finalize call whose
  blob's `env` map differs from the currently-stored blob's `env` map
  is rejected with `InvalidBlob { reason: "env_section_immutable" }`.
  The wrapper may not rewrite env via the runtime path; admin edits
  through the admin API are the only way to mutate env. Files and
  libsecret sections are freely mutable on writeback (subject to
  `writable`).
- **Server-side blob validation.** On every finalize that carries a
  blob, the backend deserializes as `ResourceBlob`, re-canonicalizes,
  and rejects non-canonical bytes with `noncanonical_blob`. Same
  contract as the admin PUT endpoint.
- **`secret_id` is gone from the runtime wire.** The runtime client
  has no way to address `__underwrap/*`, `<resource>/<slot>/<key>`,
  or any other storage key directly. The
  `ServiceWriteRequest` / `ServiceReadRequest` /
  `ServiceListRequest` / `ServiceDeleteRequest` /
  `ServiceReadPrefixesRequest` / `ServiceStatusResponse` types are
  deleted.
- **`StorageBackend` is backend-internal only.** It stays as the
  abstraction over `PlainBackend` / `EncryptedBackend` inside
  `underwrap-backendd`, but the `ServiceBackend` HTTPS client in
  `shared-store` is replaced by a typed `BackendClient` exposing only
  the three new endpoints.
- **`policy.rs`, `approval.rs`, `wrapper_flow/approval_round.rs`,
  `wrapper_flow/read.rs` (current shape), `wrapper_resource_channel.rs`
  policy re-evaluation** all collapse. The daemon retains only the
  Unix-socket wrapper-flow plumbing, plus a small typed client over
  the three HTTPS endpoints.
- **`last_used_at`** is bumped on every successful open AND every
  successful finalize, server-side. (Today it bumps on read-gate
  pass; this stays equivalent but moves into the two new handlers.)
- **No legacy cutover.** The new endpoints land alongside the old
  ones for the duration of one PR series; once the daemon is
  switched the old endpoints (`/v1/read`, `/v1/write`, `/v1/list`,
  `/v1/read-prefixes`, `/v1/delete`, `/v1/status`,
  `/v1/runtime/wrappers/register`) are deleted in the same series.
  No parallel-with-legacy intermediate stable state.

## Goals

1. The runtime HTTPS API surface is exactly three endpoints, all
   addressed by `wrapper_name` + `resource_name`. The legacy seven are
   deleted.
2. `underwrap-backendd` is the sole policy authority. It evaluates
   the policy table on every open and every finalize and answers Allow
   / Deny / InvalidBlob. The daemon carries no policy data.
3. SSH-agent per-call approval and every line of code supporting it is
   gone from both daemon and backend.
4. The daemon's `State` shrinks to `{config, BackendClient}`. No
   principal id, no resource-id map, no policy document, no pending
   approvals, no wrapper-resource channel re-evaluation.
5. The env section is admin-only; the backend independently rejects
   any runtime writeback that changes it.
6. The `__underwrap/*` runtime-write hole and the legacy
   `<resource>/<slot>/<key>` write path are closed by construction
   (the wire shape can't address them).
7. Docs, examples, the VM test, and the SPA all reflect the new model.

## Stories

- `600_story_runtime_api_redesign.md` — the single rollout story
  enumerating the task tickets below.

## Out of scope

- Optimistic concurrency / etag on finalize. Last-writer-wins on
  blobs is intentional and unchanged.
- Multi-resource invocations. One wrapper invocation = one
  `(wrapper_name, resource_name)`. The wrapper config's
  "one resource per wrapper" invariant stays.
- Bringing the SSH approval feature back through a different
  mechanism (capabilities, OS prompts, etc.). If that's wanted later
  it's a separate epic.
- Any change to the secretsd ↔ underwrapd fd-4 channel, the
  wrapper-resource Unix-socket channel between `frontend-exec` and
  `underwrapd`, or the namespace setup. Those are
  daemon-internal and stay as-is.
