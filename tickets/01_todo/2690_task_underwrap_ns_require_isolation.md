# Task 2690: require target isolation in underwrap-ns

## Goal

Collapse `underwrap-ns` from a mixed isolated/unisolated implementation to a single mandatory-isolated execution path.

## Scope

- Change target isolation resolution so the target path must exist in `/etc/underwrap/target-uids`.
  - Missing table or missing target row is a hard error.
  - Never fall back to running as the caller.
- Replace `Option<target_uid::Isolation>` with required `target_uid::Isolation` in the main execution flow.
- Always switch the child to the dedicated target UID/GID before target-facing setup and exec.
- Always use the isolated namespace hardening path:
  - PID + mount namespace
  - IPC + UTS namespace
  - session keyring separation
  - controlling TTY severing
  - inherited-fd close-on-exec sweep
- Simplify files projection:
  - staging setup/materialization happens only in the child after switching to the target UID.
  - writeback uses the single `setfsuid(X)`/`setfsgid(X)` window when needed.
  - remove the unisolated parent-materialization and explicit unmount cleanup branch if no longer needed.
- Simplify libsecret setup:
  - private bus setup always uses the isolated path.
  - remove the unisolated "hide real session bus" branch if obsolete.
- Keep all fail-closed behavior around UID/GID resolution, privilege dropping, capability clearing, and writeback.

## Acceptance criteria

- `underwrap-ns` has no runtime path that execs the target as the caller.
- A missing or invalid target UID table entry aborts before target exec.
- The target and its descendants run as the dedicated `underwrap-target-*` UID with zero capabilities.
- Existing files/libsecret writeback semantics still work under mandatory isolation.

## Suggested checks

- `cargo test -p underwrap-ns`
- `cargo test -p underwrap-protocol` if wire/config structs are adjusted
- VM test after fixtures are updated in ticket 2710
