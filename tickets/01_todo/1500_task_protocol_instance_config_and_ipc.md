# Task: Shared-protocol — instance config + IPC identity

Part of: `epic_single_instance_wrapper_redesign_2026-05`.
Blocks: every other ticket in the epic.

## Problem

The redesign collapses per-wrapper sockets/groups into one per
instance, and makes the wrapper-name a client-supplied selector
carried over IPC. Three shared-protocol surfaces change together:
the daemon config (`crates/shared-protocol/src/config.rs`), the
wrapper IPC messages (`crates/shared-protocol/src/message/wrapper.rs`),
and the exec bundle (`crates/shared-protocol/src/exec.rs`). Doing
them as one unit keeps the contract coherent.

Today (confirmed by exploration):

- `WrapperDef` carries per-wrapper `socket: PathBuf` and
  `group: String` (`config.rs:120–157`).
- `WrapperRequest::Read { scope }` carries **no** wrapper identity
  (`message/wrapper.rs:10`). Identity is socket-derived.
- The target binary path never travels on the wire — `frontend-exec`
  reads it from a per-wrapper sidecar config and passes it via argv
  to `frontend-ns`. `env_passthrough` / `env_set` likewise live only
  in the sidecar config, not in `DaemonConfig`.

## What to do

### 1. `DaemonConfig` — instance-level fields

In `crates/shared-protocol/src/config.rs`:

```rust
pub struct DaemonConfig {
    pub backend: BackendServiceConfig,
    /// Instance name; used for debug metadata and binary naming.
    pub instance: String,
    /// The single wrapper socket for this instance.
    pub socket: PathBuf,
    /// The single setgid group gating that socket.
    pub group: String,
    /// Path to the underwrap-ns helper.
    pub ns_binary: PathBuf,
    /// Path to the debug tool exec'd for `variant = Debug`.
    pub debug_tool_target: PathBuf,
    pub xdg_runtime_dir_base: String,
    pub wrappers: HashMap<String, WrapperDef>,
}
```

(`xdg_runtime_dir_base` currently lives in the sidecar config; pull
it up to instance level so the daemon can supply it in `ReadOk`.)

### 2. `WrapperDef` — trim + absorb exec params

```rust
pub struct WrapperDef {
    pub target: PathBuf,
    pub resource: String,
    #[serde(default)]
    pub env: bool,
    #[serde(default)]
    pub files: Option<FilesUse>,
    #[serde(default)]
    pub libsecret: bool,
    /// User env vars allowed through to the target.
    #[serde(default)]
    pub env_passthrough: Vec<String>,
    /// Static env vars injected into the target.
    #[serde(default)]
    pub env_set: HashMap<String, String>,
}
```

Remove `socket` and `group` (now instance-level).

**Also drop `logical_name`.** It existed because the old daemon-side
map key was the prefixed `<instance>-<name>` while the backend
catalog wanted the bare `name`. In the new model the `wrappers` map
key *is* the bare wrapper name (no per-wrapper binary to prefix), so
the key is the canonical name sent to the backend — `logical_name`
becomes redundant. Confirm against the Nix generator in ticket 1550
(it should emit `wrappers.<bare-name>`), then remove the field and
the `unwrap_or_else(|| wrapper_name.clone())` fallback in the
daemon's `register_wrapper_assets`.

### 3. Reserved name validation

Add a `DaemonConfig::validate` check (or extend the existing config
validation referenced by `startup::validate_config`) that rejects:

- a wrapper whose key is `debug` (reserved subcommand token);
- a wrapper with no projection slot enabled (preserve existing
  `wrapper_has_projection` rule).

### 4. IPC — `WrapperRequest::Read`

In `crates/shared-protocol/src/message/wrapper.rs`:

```rust
pub enum WrapperRequest {
    Read {
        wrapper_name: String,
        variant: WrapperVariant,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<String>,
    },
}
```

Define `WrapperVariant` here (it is shared by IPC and, in ticket
1510, the runtime API):

```rust
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize,
    PartialEq, Eq, Default,
)]
#[serde(rename_all = "kebab-case")]
pub enum WrapperVariant { #[default] Real, Debug }
```

(Put it somewhere both `message/` and `service.rs` can import —
e.g. a small `variant.rs` module re-exported at the crate root.)

### 5. IPC — `WrapperResponse::ReadOk` carries the exec plan

The daemon now decides the target and supplies the exec env, so
`frontend-exec` needs nothing from a sidecar:

```rust
ReadOk {
    env: HashMap<String, SecretBytes>,
    resource_id: String,
    resource_name: String,
    access: AccessMode,
    libsecret: bool,
    files_target_paths: Vec<String>,
    /// Daemon-chosen binary to exec (configured target for Real,
    /// debug tool for Debug). The wrapper never chooses this.
    target: PathBuf,
    env_passthrough: Vec<String>,
    env_set: HashMap<String, String>,
    xdg_runtime_dir_base: String,
},
```

### 6. Exec bundle

`crates/shared-protocol/src/exec.rs` `ExecBundle` already carries
`env`, `env_passthrough`, `env_set`, `xdg_runtime_dir_base`,
`resource`. Keep it as the wrapper→ns handoff, but note the target
now originates from `ReadOk.target` (passed to ns via argv, as
today) rather than a sidecar. No structural change required here
unless you choose to move the target into the bundle too — if so,
add `target: PathBuf` to `ExecBundle` and stop passing it via argv;
document the choice.

### 7. Tests

Update `crates/shared-protocol` tests for the new shapes: config
round-trip with instance-level fields, `WrapperDef` without
`socket`/`group`, `Read`/`ReadOk` serialization including
`variant` and `target`, and the reserved-`debug` validation
rejection.

## Landing note

This ticket intentionally breaks the build for `frontend-exec`,
`frontend-underwrapd`, and `backend-backendd` — they consume the
types changed here. Do **not** try to keep the workspace green at
this ticket alone; it lands as part of the `1500`–`1540` cutover
bundle (see the epic's "Landing strategy"). The workspace
`cargo build` / `cargo test --workspace` green bar is the bundle's
gate.

## Acceptance

- `cargo fmt -- --check` (on the touched files)
- `cargo test -p shared-protocol` passes in isolation (the crate
  itself compiles and its own tests are green).
- The new shapes are exercised by shared-protocol tests: config
  round-trip with instance-level fields, `WrapperDef` without
  `socket`/`group`/`logical_name`, `Read`/`ReadOk` serialization
  including `variant` and `target`, reserved-`debug` rejection.
- Clippy/`cargo test --workspace` are validated at the bundle level
  once `1520`/`1530`/`1540` are in, not here.
