# Task: Backend handlers + new wire types; delete legacy runtime API

Parent: `600_story_runtime_api_redesign.md`

## Scope

Define the new runtime API types in `shared-protocol`, implement the
three new handlers in `underwrap-backendd`, delete the seven legacy
runtime endpoints, delete the `PerCallApproval` mode from the policy
schema, drop the `files_mount_paths` field on
`WrapperResourcePairRecord`. No daemon-side work in this ticket.

### New wire types (in `shared-protocol::service`)

```rust
// POST /v1/wrappers
pub struct RuntimeWrapperRegisterRequest {
    pub wrappers: Vec<RuntimeWrapperRegisterPair>,
}

pub struct RuntimeWrapperRegisterPair {
    pub wrapper_name: String,
    pub resource_name: String,
}

pub enum RuntimeWrapperRegisterResponse {
    Ok {
        auto_created_resources: Vec<String>,            // resource names
        auto_created_pairs: Vec<RuntimeWrapperRegisterPair>,
    },
    AutoCreateDisabled {
        missing_resources: Vec<String>,
        missing_pairs: Vec<RuntimeWrapperRegisterPair>,
    },
}

// POST /v1/wrapper-invocations
pub struct RuntimeWrapperInvocationOpenRequest {
    pub wrapper_name: String,
    pub resource_name: String,
}

pub enum RuntimeWrapperInvocationOpenResponse {
    Allow {
        blob_b64: String,
        writable: bool,
    },
    Deny { reason: String },
}

// POST /v1/wrapper-invocations/finalize
pub struct RuntimeWrapperInvocationFinalizeRequest {
    pub wrapper_name: String,
    pub resource_name: String,
    pub blob_b64: Option<String>,   // Some = writeback; None = usage signal only
}

pub enum RuntimeWrapperInvocationFinalizeResponse {
    Ok,
    Deny { reason: String },
    InvalidBlob { reason: String },   // "env_section_immutable", "noncanonical_blob",
                                      // "schema_unknown", "decode_failed"
}
```

All types are tagged enums (`#[serde(tag = "outcome")]` or similar)
to keep the JSON shape unambiguous on the wire. `ts-rs` exports for
SPA consumption are not needed for runtime types — these aren't
admin-facing.

### Three new handlers (in `crates/backend-backendd/src/server.rs`)

For every handler the cert → principal resolution flow stays the
same one we already use today (`resolve_runtime_client` →
`RuntimeRequestPrincipal { instance_id, store_id }`). The store
lookup behavior is unchanged: a locked encrypted store returns the
existing `store_locked` 503.

**`POST /v1/wrappers`**:

1. Validate every `wrapper_name` is `is_valid_wrapper_name(...)` and
   every `resource_name` is `is_valid_resource_name(...)`.
2. Reuse the existing `apply_runtime_wrapper_register` logic, except
   without `files_mount_paths` (the field is gone).
3. Return `Ok { auto_created_resources, auto_created_pairs }` or
   `AutoCreateDisabled { missing_resources, missing_pairs }`.

**`POST /v1/wrapper-invocations`**:

1. Validate the wrapper-resource pair exists for this principal's
   `current_store_id` (if not, Deny `unregistered_pair`).
2. Load policy document. Find the row matching
   `(principal.instance_id, wrapper_name, resource_name)`. If
   missing → Deny `no_permission`.
3. Evaluate mode: `Locked` → Deny `locked`; `UnlockedUntil { until }`
   with `now > until` → Deny `expired`; `Unlocked` → continue. (No
   `PerCallApproval` arm — the variant is gone.)
4. Load `resources/<resource_id>` blob, canonicalize, base64.
5. Bump `last_used_at` on the matched pair record.
6. Return `Allow { blob_b64, writable: permission.writable }`.

**`POST /v1/wrapper-invocations/finalize`**:

1. Re-run steps 1 and 2 of the open path. No prior open is required;
   the call is stateless.
2. Always bump `last_used_at` on success (whether or not blob_b64 is
   present), AFTER the writeback check passes.
3. If `blob_b64` is `None`: bump `last_used_at`, return `Ok`. This is
   the usage-signal case.
4. If `blob_b64` is `Some`:
   a. If `!permission.writable` → Deny `not_writable`. (Independent
      of what the daemon thinks; this is the authoritative check.)
   b. Base64-decode.
   c. `ResourceBlob::from_canonical_bytes(&bytes)` → on err
      `InvalidBlob { reason: "decode_failed" }`.
   d. `blob.to_canonical_bytes()` != bytes →
      `InvalidBlob { reason: "noncanonical_blob" }`.
   e. Load current stored blob.
      `new_blob.env != current_blob.env` →
      `InvalidBlob { reason: "env_section_immutable" }`.
   f. `save_resource_blob(...)`. On success bump `last_used_at`,
      return `Ok`.

### Deletions

- `crates/backend-backendd/src/server.rs`:
  - `/v1/status`, `/v1/read`, `/v1/write`, `/v1/list`,
    `/v1/read-prefixes`, `/v1/delete`,
    `/v1/runtime/wrappers/register` — all route arms and their
    handlers go.
  - `runtime_read_gate`, `runtime_prefix_gate`, `bump_pair_last_used`
    callers move into the two new handlers.
  - `write_legacy_secret` and any helpers for the
    `<resource>/<slot>/<key>` shape — gone.
- `crates/shared-protocol/src/service.rs`:
  - `ServiceReadRequest`, `ServiceReadResponse`, `ServiceWriteRequest`,
    `ServiceWriteResponse`, `ServiceListRequest`, `ServiceListResponse`,
    `ServiceReadPrefixesRequest`, `ServiceReadPrefixesResponse`,
    `ServiceDeleteRequest`, `ServiceDeleteResponse`,
    `ServiceStatusResponse`, the existing
    `ServiceWrapperRegisterRequest` / `Response` —
    deleted.
- `crates/shared-protocol/src/policy.rs`:
  - `WrapperApprovalMode::PerCallApproval` variant deleted. The enum
    becomes `Locked | Unlocked | UnlockedUntil { until_epoch }`.
  - Every place that pattern-matches on `PerCallApproval` is updated.
- `crates/shared-protocol/src/entities.rs`:
  - `WrapperResourcePairRecord::files_mount_paths` field deleted.
  - The catalog migration check, the admin endpoint shapes that
    surface this field, and any tests reading it — all updated.
- `crates/backend-backendd/src/approval*` if anything ever
  existed there. Otherwise no-op.

### Acceptance criteria

- [ ] The three new routes are wired in `handle_service_request`.
- [ ] No route arm for any of the seven legacy endpoints remains.
- [ ] No `secret_id` field exists in any `shared-protocol::service`
      type that the runtime client sees.
- [ ] `WrapperApprovalMode` is `{ Locked, Unlocked, UnlockedUntil }`.
- [ ] `WrapperResourcePairRecord` has no `files_mount_paths` field.
- [ ] Each of the three handlers has at least one positive and one
      negative unit test in `server.rs` mod tests.
- [ ] `cargo test --workspace` green; clippy `-D warnings` clean.

### Out of scope

- Daemon-side work. The daemon does not compile against these new
  types until `620_task_daemon_collapse_to_thin_client.md`.
- SPA work. The admin API used by the SPA is untouched here.
