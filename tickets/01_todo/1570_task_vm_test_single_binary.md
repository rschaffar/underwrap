# Task: VM test — single-binary + debug subcommand

Part of: `epic_single_instance_wrapper_redesign_2026-05`.
Depends on: `1520`, `1530`, `1540`, `1550`.

## Problem

The VM test is the authoritative end-to-end check. It currently
invokes wrappers by their bare per-wrapper names (e.g. `test-echo`)
and asserts per-wrapper groups/sockets
(`nix/tests/test.py:508–509`, `565+`, `616–622`). After the
redesign these assumptions are gone — one binary, one group, one
socket per instance — and the debug path is a subcommand.

## What to do

### 1. `nix/tests/node.nix`

Update to the new option shape (ticket 1550): no per-wrapper groups,
one `uw-<instance>` entrypoint. `allowedUsers = [ "testuser" ]`
stays.

### 2. `nix/tests/test.py`

Rework / add:

1. **Normal invocation.** As `testuser`, run
   `uw-<inst> <wrapper> <args>` (via the PATH entrypoint). Assert it
   runs the configured target normally and the secret is projected.
   Replace the old bare-name invocations.

   Also exercise the **per-wrapper convenience shims** from ticket
   1550: run the bare `<wrapper> <args>` shim and assert it behaves
   identically to `uw-<inst> <wrapper> <args>` (argv + exit status
   forwarded), and run `debug-<wrapper>` and assert it behaves like
   `uw-<inst> debug <wrapper>` (i.e. denied under `Unlocked`, allowed
   under `UnsafeDebugUntil`). Confirm the shims are plain scripts, not
   setgid (`stat -c '%a' $(command -v <wrapper>)` has no setgid bit).

2. **Debug denied by default.** With the wrapper's permission in
   `Unlocked`, run `uw-<inst> debug <wrapper>`. Assert non-zero exit
   and the daemon log shows the backend's
   `"not in unsafe-debug mode"` deny.

3. **Debug allowed under UnsafeDebugUntil.** Drive the admin API to
   set the permission to `UnsafeDebugUntil { until_epoch = now+600 }`.
   Run `uw-<inst> debug <wrapper>`; assert exit 0 and the printed
   summary includes the projected env keys.

4. **Real stays Real under UnsafeDebugUntil.** With the permission
   still `UnsafeDebugUntil`, run `uw-<inst> <wrapper>` (no `debug`)
   and assert it runs the **configured** target, not the debug tool.

5. **Target authority.** Assert there is no way for a `Real`
   invocation to land in the debug tool — e.g. confirm the daemon
   log records `variant=real` and the exec'd target is the
   configured one. (The daemon chooses the target; this is the
   security crux.)

6. **One group, one socket.** `getent group underwrap-<inst>`
   exists; the per-wrapper groups do **not**. Exactly one wrapper
   socket exists for the instance.

7. **Unknown / reserved name rejected.** `uw-<inst> nonesuch` and
   `uw-<inst> debug` (missing name) and a config naming a wrapper
   `debug` (eval-time, covered in 1550) are rejected.

8. **Expiry.** Set `UnsafeDebugUntil { until_epoch = now-1 }`; run
   `uw-<inst> debug <wrapper>`; assert deny `"expired"`.

### 3. Spoofing resistance

Using `spoof.py` (or inline): a process **without** the instance
group egid cannot connect to the single instance socket (rejected by
the egid gate before the daemon reads the request). This replaces
the old per-wrapper spoofing assertions.

## Acceptance

- `nix build .#checks.x86_64-linux.vm-test` green with the reworked
  assertions present and exercised.
- A deliberate regression (daemon emits `debug_tool_target` for a
  `Real` request, or always-allows `Debug`) makes the new
  assertions fail — spot-check locally before merging.
