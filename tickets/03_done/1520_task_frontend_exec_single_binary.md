# Task: frontend-exec — single per-instance binary + argv dispatch

Part of: `epic_single_instance_wrapper_redesign_2026-05`.
Depends on: `1500` (IPC shapes), `1510` (`WrapperVariant`).

## Problem

`crates/frontend-exec/src/main.rs` today:

- derives wrapper identity from its **own** executable basename
  (`detect_wrapper_config_name_from_executable`, ~lines 260–298) and
  loads a **per-wrapper** sidecar config from
  `/etc/underwrap/wrappers/<name>.yaml`;
- collects `argv[1..]` and passes it straight to the target
  (`all_args`, ~line 57);
- reads `target`, `wrapper_socket`, `ns_binary`, `env_passthrough`,
  `env_set` from that sidecar (~lines 421–427, 172, 194–204);
- sends `WrapperRequest::Read { scope }` with no identity.

In the new model there is **one** binary per instance
(`uw-<instance>`). The basename now identifies the *instance*, the
first argv token may be the reserved word `debug`, the next token is
the wrapper-name, and the rest are the target's args. The daemon
returns the target + env, so the sidecar shrinks to instance-level
data (or disappears).

## What to do

### 1. Argv parsing

```text
uw-<instance> <wrapper-name> [target-args...]
uw-<instance> debug <wrapper-name> [debug-tool-args...]
```

- `argv[1] == "debug"` → `variant = Debug`, wrapper-name is
  `argv[2]`, remaining args are `argv[3..]`.
- otherwise → `variant = Real`, wrapper-name is `argv[1]`, remaining
  args are `argv[2..]`.
- Empty / missing wrapper-name → usage error.

### 2. Instance identity + config

Replace `detect_wrapper_config_name_from_executable` with
instance detection: the basename `uw-<instance>` (or `uw` for the
default instance) maps to an instance config that carries the
single `socket`, `ns_binary`, and `xdg_runtime_dir_base`. The
per-wrapper sidecar files are gone — `frontend-exec` no longer needs
`target` / `resource` / `env_*` locally; those arrive in `ReadOk`.

Keep `ensure_underwrap_group_from_egid()` — it still asserts the
process holds the instance group egid before talking to the daemon.

### 3. IPC request

Send:

```rust
WrapperRequest::Read {
    wrapper_name,        // from argv
    variant,             // Real | Debug
    scope: None,
}
```

### 4. Exec via ns using the daemon's answer

`ReadOk` now carries `target`, `env_passthrough`, `env_set`,
`xdg_runtime_dir_base`. Build the `ExecBundle` from the **response**
(not a sidecar), and exec `ns_binary` with the **`ReadOk.target`**
as the target argv and the parsed target-args after it. For
`variant = Debug` the daemon will have set `target` to the debug
tool and populated the `UNDERWRAP_DEBUG_*` keys in `env_set` — do
not special-case the tool in `frontend-exec`; just honor the
response.

### 5. Privilege drop unchanged

`drop_privilege()` and the existing fd hygiene
(`close_channel_fd_for_exec`, etc.) are unchanged.

### 6. Tests — `crates/frontend-exec/src/main/tests.rs`

The existing fixtures hard-code per-wrapper sidecar shapes; rework
them to the instance-config shape. Add argv-parsing tests:

- `["gh-authorized"]` → `(Real, "gh-authorized", [])`
- `["gh-authorized", "pr", "list"]` → `(Real, "gh-authorized", ["pr","list"])`
- `["debug", "gh-authorized"]` → `(Debug, "gh-authorized", [])`
- `["debug", "gh-authorized", "shell"]` → `(Debug, "gh-authorized", ["shell"])`
- `[]` and `["debug"]` → usage error.

## Out of scope

- Daemon side that validates the name and chooses the target
  (ticket 1530).

## Acceptance

- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cargo test -p frontend-exec`
- A hand-run `uw-<inst> <wrapper> ...` against a dev daemon execs
  the configured target; `uw-<inst> debug <wrapper>` execs the
  debug tool only when the daemon allows it.
