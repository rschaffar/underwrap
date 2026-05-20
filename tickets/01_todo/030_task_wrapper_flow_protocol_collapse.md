# Task: collapse wrapper-flow protocol to a single binding

Parent: [[000_story_one_resource_per_wrapper_frontend]]
Depends on: [[010_task_flatten_wrapper_config_schema]]

## Goal

The wrapper-flow IPC carries a `Vec<ReadResourceBinding>` and sends N
SCM_RIGHTS fds. With one resource per wrapper, this collapses to a
single inline binding and a single channel fd.

## Changes

### `crates/shared-protocol/src/message/wrapper.rs`

- Delete `ReadResourceBinding` (struct + `WireEncode`/`WireDecode` impls).
- `WrapperResponse::ReadOk` becomes:
  ```rust
  ReadOk {
      env: std::collections::HashMap<String, SecretBytes>,
      resource_id: String,
      access: AccessMode,
      libsecret: bool,
      files_target_paths: Vec<String>,
  }
  ```
  …or, if the four resource-scoped fields cluster naturally, keep them
  in a small `ResourceBinding` struct *embedded by value* in `ReadOk`
  rather than a `Vec`. Either is fine — pick the one that reads better
  in the wrapper consumer (`frontend-exec`). The constraint is "no
  vector of bindings on the wire."
- Update `WireEncode`/`WireDecode` for `WrapperResponse::ReadOk`
  accordingly. Encode order locked: discriminant, env, resource_id,
  access, libsecret, files_target_paths.

### `crates/frontend-exec/src/main.rs`

- The consumer of `ReadOk` no longer iterates `resources`; reads the
  one binding directly.
- Handoff to stage 2 (`underwrap-ns`) still carries the resource_id /
  access / libsecret / files info, but as a single set, not a list.

### `crates/frontend-ns/src/main.rs`

- Any code that walks a list of resource bindings collapses to the
  single binding shape. The libsecret launch path no longer needs to
  *find* a libsecret resource — it's a bool on the binding.

### `crates/shared-protocol/src/message/wrapper_resource.rs`

- Module doc comment at the top ("one socketpair per resource") is
  updated. The channel itself is unchanged on the wire; only the
  *count per wrapper read* changes (always one).

### Daemon side

- `wrapper_flow/channels.rs` already updated by
  [[020_task_frontend_daemon_collapse]] to send one fd; this task
  confirms the wire shape matches.

### Tests

- Wire round-trip tests for `WrapperResponse::ReadOk` are updated to
  the new shape.
- The integration test in `read.rs:237-288` that asserts
  `resources.len() == 2` is rewritten or deleted (multi-resource is no
  longer expressible). Replace it with a test that covers env + files
  + libsecret on a single resource if such a test does not already
  exist elsewhere.

## Acceptance

- `cargo test --workspace` passes.
- `cargo clippy --all-targets --workspace -- -D warnings` passes.
- The wrapper-flow integration test (`cargo test -p integration-tests`)
  passes against a real daemon.
- `nix build .#checks.x86_64-linux.vm-test` passes (see
  [[040_task_docs_nixos_module_vm_test]] for VM config updates).
