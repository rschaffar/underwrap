# Task: Collapse `underwrapd` to a thin client over the three new endpoints

Parent: `600_story_runtime_api_redesign.md`

## Scope

Replace the daemon's policy / approval / resource-id / principal-id
machinery with a small typed client over the three new endpoints from
`610_task_backend_handlers_and_protocol_types.md`. Every line of code
that exists only because policy or approval lived client-side gets
deleted. The wrapper-flow shrinks to "open → relay blob over the
wrapper-resource channel → finalize".

### New `BackendClient` (replaces `ServiceBackend`)

In a new module `crates/frontend-underwrapd/src/backend_client.rs`
(or under `shared-store` if cleaner — coordinate with the protocol
crate location), define a typed reqwest-based client with three
methods:

```rust
pub struct BackendClient { /* reqwest::Client, base_url */ }

impl BackendClient {
    pub fn open(config: &BackendServiceConfig) -> Result<Self>;
    pub fn register_wrappers(&self, req: &RuntimeWrapperRegisterRequest)
        -> Result<RuntimeWrapperRegisterResponse>;
    pub fn open_invocation(&self, req: &RuntimeWrapperInvocationOpenRequest)
        -> Result<RuntimeWrapperInvocationOpenResponse>;
    pub fn finalize_invocation(&self, req: &RuntimeWrapperInvocationFinalizeRequest)
        -> Result<RuntimeWrapperInvocationFinalizeResponse>;
}
```

`ServiceBackend` and the `StorageBackend` trait impl for it in
`shared-store/src/service.rs` are **deleted**. `StorageBackend` stays as
a backend-internal abstraction (used only inside
`underwrap-backendd`); `shared-store` becomes a backend-internal
crate.

### New `State`

```rust
pub(crate) struct State {
    pub(crate) config: DaemonConfig,
    backend: std::sync::Mutex<Option<Arc<BackendClient>>>,
}
```

Lazy-init pattern preserved (the self-heal across backend restarts we
just shipped). `ensure_enrolled` becomes `ensure_backend_client` and
returns `Result<Arc<BackendClient>, EnrollmentError>`. On first
successful open the client is cached. `EnrollmentError::Backend(...)`
is what the daemon emits when the register call 401s.

### Deletions

- `crates/frontend-underwrapd/src/policy.rs` — entire module.
- `crates/frontend-underwrapd/src/approval.rs` — entire module.
- `crates/frontend-underwrapd/src/wrapper_flow/approval_round.rs` —
  entire module.
- The policy re-evaluation in
  `crates/frontend-underwrapd/src/wrapper_resource_channel.rs`
  (load_policy_document + writable_for) — gone. Writable enforcement
  is purely server-side on finalize.
- The `client_instance`, `resource_ids` fields and
  `resource_id_for_wrapper` accessor on `Enrolled`/`State` — gone.
- The `register_wrapper_assets` function in `server.rs` — replaced by
  `BackendClient::register_wrappers`.
- The `<resource>/<slot>/<key>` legacy expansion in
  `crates/shared-store/src/resource_blob.rs::expand_blob_to_entries` and
  `blob_from_resource_entries` — gone if no remaining caller; keep
  only what `secretsd` and the wrapper-resource channel still need
  in-memory.

### Wrapper-flow rewrite

`crates/frontend-underwrapd/src/wrapper_flow/mod.rs::complete_read`
becomes:

```rust
async fn complete_read(...) -> Result<ReadResult> {
    let client = state.ensure_backend_client()?;
    match client.open_invocation(&RuntimeWrapperInvocationOpenRequest {
        wrapper_name: wrapper_name.into(),
        resource_name: state.config.wrappers[wrapper_name].resource.clone(),
    })? {
        Allow { blob_b64, writable } => /* ship blob to wrapper-resource channel */,
        Deny { reason } => /* WrapperResponse::Error { Unauthorized, reason } */,
    }
}
```

`channels.rs::establish_scoped_channels` keeps its socketpair / fd-3
plumbing; only the `ChannelContext` shrinks. `wrapper_resource_channel.rs`
keeps `Checkout` / `Checkin` / `UsageSignal` IPC verbs unchanged (the
secretsd protocol relies on them), but `handle_checkin` and
`handle_usage_signal` route to `client.finalize_invocation(...)` with
optional blob instead of writing directly. `handle_checkout` ships the
blob the daemon already received on open — no fresh backend call.

### Daemon-side env-immutability sanity check (defense in depth)

When the wrapper does Checkin, the daemon ALREADY has the original
blob from the open call. Before forwarding to
`finalize_invocation`, the daemon can compare `new_blob.env` to
`original_blob.env` and short-circuit with the existing "writeback
rejected" warning if they differ. This is cheaper than a round-trip
and gives a clear daemon-side log. The backend re-checks anyway. **Do
both.**

### Dependency hygiene

Remove from `crates/frontend-underwrapd/Cargo.toml`:

- `ssh-key`
- `signature`
- `ed25519-dalek`
- any other transitive that exists only for the approval flow

Run `cargo machete` or grep for unused dev-deps in the daemon crate.

### Acceptance criteria

- [ ] `crates/frontend-underwrapd/src/policy.rs` does not exist.
- [ ] `crates/frontend-underwrapd/src/approval.rs` does not exist.
- [ ] `crates/frontend-underwrapd/src/wrapper_flow/approval_round.rs`
      does not exist.
- [ ] `State` is two fields: `config` and the lazy backend client.
- [ ] No reference to `PolicyDocument`, `WrapperPermission`,
      `client_instance` (as a daemon field), or `resource_id` (as a
      daemon-known string) remains in `frontend-underwrapd`.
- [ ] The wrapper-resource channel's `handle_checkin` and
      `handle_usage_signal` route through `BackendClient`.
- [ ] `frontend-underwrapd/Cargo.toml` no longer pulls `ssh-key`,
      `signature`, or `ed25519-dalek`.
- [ ] `cargo test --workspace` green; clippy `-D warnings` clean.

### Out of scope

- SPA work — `630_task_spa_remove_approval_and_mount_paths.md`.
- Integration / VM tests — `640_task_integration_tests.md`,
  `650_task_vm_test_update.md`.
- Docs — `660_task_docs.md`.
