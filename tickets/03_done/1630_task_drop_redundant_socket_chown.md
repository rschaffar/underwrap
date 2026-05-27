# Task: Drop the redundant wrapper-socket `chown` (and tighten the syscall filter)

Part of: hardening follow-up (post-2026-05 epics). Low priority.

## Problem

`startup::bind_socket` unconditionally `chown`s the wrapper socket to
the instance gid (uid unchanged). But the daemon's **primary group is
already the instance group** — the NixOS module sets the daemon user's
`group = daemonGroup` and `instanceGroup = daemonGroup`
(`nix/modules/underwrap.nix`). So a socket the daemon binds is already
group-owned by the instance group, and the `chown(uid=-1, gid=instance)`
is a no-op in the standard deployment.

Its only real consequence is forcing the systemd
`SystemCallFilter` to re-allow `@chown` after `~@privileged` — i.e. the
daemon keeps a syscall it does not actually need.

(The integration-test harness runs the daemon with
`group = current_group_name()`, which also matches the daemon's primary
group, so the chown is redundant there too.)

## What to do

- Verify across all run modes (NixOS module, integration-test harness)
  that the daemon's primary group always equals the socket's intended
  group. If so:
  - drop the `chown` from `bind_socket` (rely on the daemon's primary
    group + the `0660` mode + umask), and
  - remove `"@chown"` from the `SystemCallFilter` in
    `nix/modules/underwrap.nix`, leaving
    `[ "@system-service" "~@privileged" ]`.
- If there is any mode where the primary group differs, leave the chown
  but document why (and keep `@chown`).

## Acceptance

- `nix build .#checks.x86_64-linux.vm-test` green (socket is still
  group `underwrap-<instance>`, mode `0660`, and wrappers connect).
- `cargo test --workspace` green.
- If the chown is dropped, the syscall filter no longer re-allows
  `@chown`.
