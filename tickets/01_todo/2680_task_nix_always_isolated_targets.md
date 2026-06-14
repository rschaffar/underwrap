# Task 2680: Nix module always isolates targets

## Goal

Remove the per-wrapper `isolate` option from the NixOS module and make target UID isolation mandatory for every configured wrapper target.

## Scope

- Remove `services.underwrap.instances.<name>.wrappers.<wrapper>.isolate`.
- Compute the target UID table from every distinct configured wrapper `target`, not only `w.isolate` targets.
- Always create the per-target `underwrap-target-*` users/groups for configured targets.
- Always write `/etc/underwrap/target-uids` when underwrap is enabled and wrappers exist.
- Make `underwrap-ns` capabilities unconditional for the mandatory-isolation model:
  - `cap_sys_admin,cap_setpcap,cap_setuid,cap_setgid+ep`
- Remove `anyTargetIsolated` branching and comments that describe optional isolation.
- Update Nix assertions/options help text accordingly.

## Acceptance criteria

- There is no Nix option named `isolate`.
- Every configured target path has exactly one row in `/etc/underwrap/target-uids`.
- Wrappers sharing the same absolute target path still share the same dedicated target user/group.
- No unisolated target can be configured through the module.

## Suggested checks

- `nix eval` or targeted module evaluation if available.
- `cargo test -p underwrap-protocol` if generated config shape changes touch protocol structs.
- Full VM test after the follow-up test/doc ticket updates the fixtures.
