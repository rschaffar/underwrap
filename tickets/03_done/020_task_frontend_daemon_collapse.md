# Task: collapse the frontend daemon to one resource per wrapper

Parent: [[000_story_one_resource_per_wrapper_frontend]]
Depends on: [[010_task_flatten_wrapper_config_schema]]

## Goal

Remove every loop and helper in `frontend-underwrapd` that iterates
"resources of a wrapper". With the schema change, each wrapper has
exactly one resource — most of these become a single lookup.

## Changes

### `crates/frontend-underwrapd/src/wrapper_bindings.rs`

Delete:

- `wrapper_libsecret_resource`
- `wrapper_env_resources`
- `wrapper_file_bindings`
- `wrapper_all_resources`
- `FileProjection`
- `resource_has_projection`

Replace with whatever single helper(s) the call sites actually need. A
likely shape:

```rust
pub(crate) fn wrapper_file_paths(wrapper: &WrapperDef) -> Vec<String> {
    wrapper.files.as_ref().map(|f| f.paths.clone()).unwrap_or_default()
}
```

…but inline at call sites where one access is enough.

### `crates/frontend-underwrapd/src/wrapper_flow/read.rs`

- The three sequential loops (env, files, libsecret) over per-slot
  helpers collapse to a single block that consults the wrapper's one
  resource and decides what to project.
- `resource_ids: Vec<String>` and the per-id dedup logic go away.
- The "duplicate env var across resources" check at line 159 is
  unreachable — delete it (with its error path and test, if any).
- `WrapperResponse::ReadOk` is constructed with a single binding (see
  the protocol-collapse task for the exact new shape).

### `crates/frontend-underwrapd/src/wrapper_flow/approval_round.rs`

- `wrapper_all_resources` call goes away; the approval round consults a
  single grant for the wrapper's one resource.

### `crates/frontend-underwrapd/src/wrapper_flow/channels.rs`

- `setup_wrapper_resource_channels` (the loop over `resource_access`)
  collapses to one channel setup. SCM_RIGHTS sends exactly one fd.
- Module doc comment at the top ("one fd per `resources` entry") is
  updated.

### `crates/frontend-underwrapd/src/startup.rs`

- The validation loop iterating `wrapper.resources` (line 77) becomes a
  straight-line check on `wrapper.files`, `wrapper.env`,
  `wrapper.libsecret`. The non-empty-resources assertion at line 70
  becomes "wrapper has at least one slot enabled" (or is removed if
  schema validation already enforces it).

### `crates/frontend-underwrapd/src/server.rs`

- The pair-register inner loop at line 56 iterates resources to enumerate
  pairs; collapse to a single `(wrapper, resource)` pair per wrapper.

## Acceptance

- `cargo test --workspace` passes.
- `cargo clippy --all-targets --workspace -- -D warnings` passes.
- No reference to `wrapper.resources` or `ResourceUse` remains in
  `frontend-underwrapd`.
- Daemon still emits one `read_granted` audit event per wrapper read
  (resource id still on the event).
