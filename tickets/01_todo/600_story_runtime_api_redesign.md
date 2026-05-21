# Story: Runtime API redesign — resource-vocabulary, server-side authorization

Parent: `epic_runtime_api_redesign.md`

## Outcome

After this story:

- The runtime HTTPS API between `underwrapd` and `underwrap-backendd`
  is exactly three endpoints, all addressed by `wrapper_name` and
  `resource_name`. There is no `secret_id` on the wire, no `/v1/read`,
  no `/v1/write`, no `/v1/list`, no `/v1/read-prefixes`,
  no `/v1/delete`, no `/v1/status`.
- `underwrap-backendd` is the sole policy authority. The daemon holds
  no policy state, knows no principal id, knows no resource ids, and
  carries no SSH crypto.
- The `PerCallApproval` mode, the daemon's `approval` module, the
  `ssh-key` / `signature` / `ed25519-dalek` dependencies, the
  `WrapperRequest::ApprovalProof` / `WrapperResponse::ApprovalRequired`
  variants, and the SPA's per-call approval UI are deleted.
- The env section of a resource blob is admin-only; the backend
  rejects any runtime writeback that mutates it.
- The legacy `<resource>/<slot>/<inner>` write path and the
  `__underwrap/*` runtime-write hole are closed by construction.
- `WrapperResourcePairRecord` no longer carries `files_mount_paths`.
- Docs, examples, CLAUDE.md, the VM test, and the SPA all reflect the
  new model.

## Tasks

- `610_task_backend_handlers_and_protocol_types.md` — new wire types,
  three new handlers, delete the seven legacy runtime endpoints,
  delete `PerCallApproval` from the policy schema, drop the
  approval module on the backend side, drop the `files_mount_paths`
  field from `WrapperResourcePairRecord`.
- `620_task_daemon_collapse_to_thin_client.md` — collapse `State`,
  delete `policy.rs` / `approval.rs` / `wrapper_flow/approval_round.rs`
  / the policy re-eval in `wrapper_resource_channel.rs`, replace
  `ServiceBackend` with a typed `BackendClient` exposing only the
  three new endpoints, simplify the wrapper-flow to "open → relay →
  finalize", drop the `<resource>/<slot>/<inner>` expansion entirely.
- `630_task_spa_remove_approval_and_mount_paths.md` — drop the
  per-call-approval UI in the wrapper-permissions page, drop the
  mount-paths column in the Secrets browser, replace it with grouping
  derived from file-section keys, remove approval-related TypeScript
  bindings.
- `640_task_integration_tests.md` — full coverage of the three new
  endpoints, the env-immutability rule, the daemon-bypass tests for
  writes, the no-storage-key-leakage property, and concurrent
  invocations.
- `650_task_vm_test_update.md` — drive the NixOS VM test through the
  new API end-to-end; assert no SSH approval code paths remain.
- `660_task_docs.md` — `docs/architecture.md`, `docs/backend.md`,
  `docs/security.md`, `CLAUDE.md`, `examples/config.yaml`, and
  `docs/getting-started.md` updates.

## Acceptance criteria

- [ ] `grep -rn "PerCallApproval\|ApprovalProof\|ApprovalRequired\|approval_key" crates/ web/` returns no hits outside of historical ticket files.
- [ ] `grep -rn "ServiceBackend\|secret_id\|/v1/read\|/v1/write\|/v1/list\|/v1/read-prefixes\|/v1/delete\|/v1/status" crates/` returns no hits in production code.
- [ ] `frontend-underwrapd/Cargo.toml` no longer depends on `ssh-key`, `signature`, or `ed25519-dalek`.
- [ ] `cargo test --workspace` green; clippy `-D warnings` clean; `cargo fmt --check` clean.
- [ ] `nix build .#checks.x86_64-linux.vm-test` green.
- [ ] The daemon's `State` is `{ config, BackendClient }` (plus whatever the wrapper-flow plumbing needs that has nothing to do with backend state).
- [ ] A runtime mTLS client cannot, via the wire shape alone, address `__underwrap/*` or `<resource>/<slot>/<key>` keys.
- [ ] A finalize call whose blob mutates the env section is rejected with `InvalidBlob { reason: "env_section_immutable" }`.
