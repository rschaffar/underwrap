# Task: Daemon — single socket, name validation, variant relay, target authority

Part of: `epic_single_instance_wrapper_redesign_2026-05`.
Depends on: `1500` (config + IPC), `1510` (`WrapperVariant`).

## Problem

The daemon today binds **one socket per wrapper** and ties each
accept loop to a fixed `wrapper_name` + `expected_gid`
(`crates/frontend-underwrapd/src/server.rs:189–250`), deriving
identity purely from the socket + egid
(`wrapper_flow/intake.rs:15–64`). In the new model there is **one
socket per instance**; the wrapper-name arrives in the IPC request
and must be validated against config, and the daemon — not the
wrapper — decides the target to exec.

## What to do

### 1. Single socket bind — `server.rs` + `startup.rs`

Replace the per-wrapper bind loop with a single bind:

```rust
let gid = resolve_group_gid(&config.group)?;
let listener = bind_socket(&config.socket, 0o660, "wrapper", Some(gid))?;
tasks.spawn(accept_wrapper_loop(listener, Arc::clone(&state), gid));
```

`build_wrapper_gid_map` collapses to a single instance gid (keep the
`getgrnam` resolution; drop the per-wrapper map and its
uniqueness check).

### 2. Intake — egid gate, then validated name

`accept_wrapper_loop` / `handle_wrapper` no longer carry a fixed
`wrapper_name`. The peer check keeps asserting
`peer.egid == instance_gid` (anti-spoofing: only processes that went
through the setgid binary can talk to us). After the egid gate, read
the first `WrapperRequest::Read { wrapper_name, variant, .. }` and:

- reject `wrapper_name == "debug"` and any name **not** present in
  `config.wrappers` with `ErrorCode::Unauthorized` /
  a structured `"unknown wrapper"` error;
- look up the `WrapperDef` for the validated name.

`authorize_wrapper_peer` drops its `wrapper_name`/`expected_gid`
specialization down to a single `expected_gid` (the instance gid).

### 3. Relay to backend with variant

The wrapper-flow open call (`wrapper_flow/…`, `backend_client.rs`)
sends the validated `wrapper_name` + `wrapper.resource` + `variant`
to `POST /v1/wrapper-invocations`. The backend returns Allow/Deny
per the matrix (ticket 1540). Thread `variant` through to the
finalize call as well (it must match the open call for the channel
lifetime — store it on the resource-channel struct, as before).

### 4. Daemon chooses the target → `ReadOk`

On Allow, build `WrapperResponse::ReadOk` with:

- `target`:
  - `variant = Real` → `wrapper.target`
  - `variant = Debug` → `config.debug_tool_target`
- `env_passthrough` / `env_set` from the `WrapperDef`
  (for `Debug`, **additionally** inject the `UNDERWRAP_DEBUG_*`
  metadata keys — `UNDERWRAP_DEBUG_INSTANCE = config.instance`,
  `UNDERWRAP_DEBUG_LOGICAL_WRAPPER` / `UNDERWRAP_DEBUG_WRAPPER` =
  wrapper name(s), `UNDERWRAP_DEBUG_ORIGINAL_TARGET = wrapper.target`);
- `xdg_runtime_dir_base` from `config`;
- the existing resource fields (`resource_id`, `resource_name`,
  `access`, `libsecret`, `files_target_paths`).

This is the security crux: the user cannot obtain the debug-tool
target under a `Real` authorization, because the daemon only emits
the debug-tool target when it asked the backend with
`variant = Debug` and got Allow.

### 5. Registration

`register_wrapper_assets` is unchanged in spirit — it still
registers each `(wrapper_name, resource)` pair from `config.wrappers`
with the backend. There are no debug entries to skip (debug is a
variant, not a config entry).

### 6. Tests

- `server`/`startup` tests: single-socket bind, single gid.
- intake tests: unknown wrapper-name → error; `debug` name → error;
  valid name + `variant = Debug` → backend called with `Debug` and,
  on Allow, `ReadOk.target == debug_tool_target`; `variant = Real` →
  `ReadOk.target == wrapper.target`.
- Fake the backend client to assert the relayed `(wrapper_name,
  resource, variant)` shape.

## Out of scope

- Backend matrix itself (ticket 1540).
- frontend-exec argv (ticket 1520).

## Acceptance

- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cargo test -p frontend-underwrapd`
- `cargo test -p integration-tests` (single-socket flow, both
  variants, name validation).
