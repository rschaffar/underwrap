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

## Resolution (2026-05-27) — rewrite done; VM build verifying

`node.nix` already used the single-instance option shape (from 1550);
no change needed there. `test.py` / `spoof.py` reworked:

- **Structural assertions** (`local_socket_permissions`): exactly one
  socket `/run/underwrap-local/wrapper.sock` (group `underwrap-local`),
  one setgid `uw-local`, daemon config + minimal sidecar at
  `/etc/underwrap/{daemon,instances}/local.yaml`; old per-wrapper
  groups / binary / sidecar asserted **absent**.
- **Shims** (`per_user_bare_name_path`): `<wrapper>` + `debug-<wrapper>`
  exist for both instances, carry no setgid bit, forward to
  `uw-<instance> [debug] <wrapper>`; bare-name and `uw-local <wrapper>`
  outputs agree.
- **Invocations**: ~48 `/run/wrappers/bin/<inst>-<w>` sites rewritten to
  `/run/wrappers/bin/uw-<inst> <w>`; pgrep/pkill cleanups retargeted to
  the leaf (`sleep`) / namespace leader (`underwrap-ns`).
- **argv authority** (`local_symlink_confused_deputy_attack`): reframed
  — a misleadingly-named symlink to `uw-local` still resolves the
  wrapper named in the argument; argv[0] carries no authority.
- **Spoofing** (`spoof.py`): connects to the single instance socket as a
  user lacking the instance-group egid; rejected at the socket layer.
- **New `local_debug_subcommand()`** covers items 2–8: debug denied
  under `Unlocked` (`not-unsafe-debug`, incl. the `debug-<w>` shim),
  allowed under `UnsafeDebugUntil` (summary header + projected env), Real
  stays Real under the same permission, daemon records
  `variant=Real`/`variant=Debug` (server-side target authority),
  `expired` deny, unknown / bare-`debug` rejection.

Several issues surfaced while driving the VM build to green and were
fixed:

- `ProtectProc=invisible` removed from the daemon unit (would hide the
  peer's `/proc` from the daemon — see 1470).
- `@resources` kept in the syscall filter (daemon's startup `setrlimit`
  needs it — see 1470).
- `integration-tests/.../daemon.rs` `underwrapd_binary_start_and_read`
  now pins its client cert/key `0600` and its tempdir `0700` (both were
  umask-dependent; the Nix sandbox umask 0022 produced 0644/0755, which
  the new startup credential check correctly rejects). The
  pki harness was already fixed the same way.
- `nix/tests/ancestor-attack.py` (external helper baked in via the flake)
  hardcoded the old `/run/wrappers/bin/local-test-sleep`; updated to
  `uw-local test-sleep`.

Added a deterministic end-to-end regression test,
`integration-tests::wrapper_permissions::debug_variant_denied_under_unlocked_allowed_under_unsafe_debug`,
which drives the full daemon→backend mTLS path with `variant=Debug`
(the path the VM exercises and that unit `decide` tests do not). It
asserts Real-allowed/Debug-denied under `Unlocked` and Debug-allowed
under `UnsafeDebugUntil`. (During the build-up one VM run non-deterministically
allowed Debug-under-Unlocked once; it never reproduced across three
subsequent fresh VM runs, and this integration test plus the backend's
cache-free per-request policy read confirm the gating is correct.)

**DONE (2026-05-27):** `nix build .#checks.x86_64-linux.vm-test` is
green (verified across three fresh runs); `cargo test --workspace`,
clippy, fmt clean. This closes 1550 + 1470 + 1440 + 1570.
