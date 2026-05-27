# Task: Close the resource-channel fd in libsecret helper forks

Part of: `epic_frontend_security_hardening_2026-05`.

## Problem

`crates/frontend-ns/src/libsecret.rs:setup_libsecret` forks both
`dbus-daemon` and `underwrap-secretsd` from inside `run_child` (which
already holds the wrapper-resource channel fd as part of the
`ResourceChannel` struct). Both child branches explicitly close:

- `secret_fd` (the exec-bundle pipe, fd 3)
- the readiness pipe ends (`ready_read`, `ready_write`)
- the dbus-address pipe ends (`dbus_addr_read`, `dbus_addr_write`)
- the libsecret init pipe ends (`init_read`, `init_write`)

They do **not** close the wrapper-resource channel fd. That fd was
placed at `RESOURCE_CHANNEL_FD = 20` by `frontend-exec` via `dup2`
(which clears `FD_CLOEXEC`), and the parent's later `dup(fd)` for
each Checkout / Checkin op also returns a non-CLOEXEC dup. Neither
helper child closes it before `exec`, so both `dbus-daemon` and
`secretsd` inherit fd 20.

The "Descriptor hygiene" invariant in `docs/security.md` and the
"Never do" list in `CLAUDE.md` both state: *"Never pass the secretsd
channel fd (fd 4) to the target process — close it before exec'ing
the target."* The wrapper does close it before the target exec
(`resource_channel::close_channel_fd_for_exec`), but leaves it open
across the two helper execs.

This is realistically exploitable only via the chain:

1. Caller-controlled `XDG_DATA_DIRS` (see ticket 1410) reaches
   `dbus-daemon`.
2. A caller-supplied `.service` file in that path claims some bus
   name the target ever calls.
3. dbus-daemon auto-activates that service inside the namespace.
4. That auto-activated process walks `/proc/self/fd`, finds fd 20,
   and replays Checkin (or Checkout, depending on writability)
   against the daemon.

Ticket 1410 closes step 1. Closing the fd here closes step 4
independently — defence-in-depth that aligns the implementation
with the stated invariant.

## What to do

In `crates/frontend-ns/src/libsecret.rs`:

1. Change `setup_libsecret`'s signature to take the resource-channel
   fd as an explicit `channel_fd: i32` parameter. Pass it from
   `run_child` (it's already available there as
   `resource_channel.channel_fd`).

2. In the `dbus_pid == 0` branch, add `libc::close(channel_fd)`
   alongside the existing `libc::close(secret_fd)`.

3. In the `secretsd_pid == 0` branch, do the same.

4. Confirm the parent path is unchanged — the parent still holds the
   fd for the eventual writeback dispatch in
   `crates/frontend-ns/src/writeback.rs`.

5. Add a unit test (or wire one into `tests.rs` next to libsecret.rs)
   that asserts both helper children's `/proc/self/fd` does not
   contain the channel fd after their execs. A simple shape: stub
   `dbus_daemon` and `secretsd` to small shell scripts that print
   `ls /proc/self/fd`, and check the channel fd is absent. The
   integration-tests crate has the fixtures for this.

## Related

The same issue does **not** occur for the target exec: the wrapper
already calls `close_channel_fd_for_exec` immediately before
`cmd.exec()` in `run_child`. This ticket extends that hygiene to
the two helpers.

## Acceptance

- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`
- New test: dbus-daemon and secretsd child both lack the channel fd
  in `/proc/self/fd` immediately after exec.
- VM test still green: `nix build .#checks.x86_64-linux.vm-test`.
- The "Descriptor hygiene" section of `docs/security.md` updated to
  state the fd is closed before *all three* execs (target, dbus,
  secretsd), not just the target.
