# Task: Comprehensive VM test proving target + descendant isolation end to end

Parent: `2600_story_target_uid_isolation`
Depends on: `2630`, `2640`, `2650`

## Goal

A single decisive VM-test section that proves the whole property holds together:
a same-UID caller (and processes it spawns) cannot inspect an isolated target or
any of its descendants, root still can, and the target remains fully functional
(home rw, sockets, libsecret, writeback). This replaces the old
`local_pid_namespace_proc_isolation` + `local_ancestor_attack` confidence with a
real matrix, and is the gate before flipping production instances (e.g.
`aws-sso`) to `isolateTargetUid = true`.

## Fixtures (in `nix/tests/node.nix`, on the isolated instance)

- `test-iso-sleep` — `sleep 300`, `env = true`, secret
  `ISO_SECRET=iso_secret_val` in its env. Long-lived target for `/proc` probing.
- `test-iso-tree` — a script that `exec`s a child `sleep 300` (descendant), so we
  can probe the **child's** `/proc`. Secret in env.
- `test-iso-home` / `test-iso-id` — reuse from 2640/2650 for the functional half.
- Keep an **unisolated** `test-sleep` for the contrast assertions.

## Test matrix (`nix/tests/test.py`, new `local_target_uid_isolation()` called
from the run sequence)

For the isolated target **and** its spawned descendant, as `testuser` (uid 1099,
the caller's uid):

1. `cat /proc/<pid>/environ` → **EPERM**, and the secret `iso_secret_val` is
   never recoverable.
2. `dd if=/proc/<pid>/mem` → **EPERM**.
3. `cat /proc/<pid>/maps` → **EPERM**.
4. `cat /proc/<pid>/auxv` → **EPERM**.
5. `ptrace(PTRACE_ATTACH)` → **EPERM** (reuse the `ancestor-attack.py` harness as
   both same-uid peer and ancestor).
6. `process_vm_readv` of the target → **EPERM** (small C/python helper).
7. **Contrast:** the same probes against unisolated `test-sleep` show `environ`
   readable (so a regression that silently disables isolation is caught — the
   test must see the difference, not a uniform "denied").

Root-can-still-read (document the boundary, don't pretend otherwise):

8. As **root**, `cat /proc/<pid>/environ` of the isolated target **succeeds**
   (CAP_SYS_PTRACE bypasses the uid gate). Assert success and log it as the
   documented limitation, so the test encodes the true threat model.

Identity + functionality (ties in 2640/2650):

9. Read as root: isolated target's real uid == X (the dedicated uid).
10. Inside the target: `getuid()==geteuid()==1099` (2650).
11. Home rw: target reads a pre-seeded `$HOME` file and writes one; the new file
    is on-disk owned by `testuser`; host `$HOME` ownership/mode unchanged (2640).
12. Socket: target `connect()`s a `testuser`-owned `0700` socket in
    `$XDG_RUNTIME_DIR` (2640).
13. libsecret still works: the existing `test-libsecret` store/lookup scenario
    passes when its instance has `isolateTargetUid = true` (dbus-daemon +
    secretsd now run as X; private bus still reachable by the target, still
    invisible to the host — re-assert the "no bus socket on host" check).
14. Writeback still works: a `files`-slot wrapper on the isolated instance
    persists changes; the parent (`underwrap-ns` supervisor, still caller-uid)
    completes checkin; on-disk artifacts owned by the caller via idmap.
15. Capabilities: isolated target reports all-zero Eff/Prm/Inh/Amb caps.

Signal semantics (note, light assertion):

16. With real=eff=saved=X, `testuser` cannot `kill` the isolated target
    (`kill_ok_by_cred` requires matching uid/suid) — assert EPERM on `kill` from
    `testuser`, and that **root** (or tty SIGINT) can still terminate it. Keep
    this informational if it proves flaky for interactive vs. plain targets.

## Acceptance

- `local_target_uid_isolation()` is wired into the test run and asserts the full
  matrix above (hard asserts on 1–7, 9–15; 8 asserts root-success; 16 best-effort
  with clear logging).
- No escape-hatch PASS paths: if a target/descendant can't be located by root,
  the test fails (not "invisible ⇒ pass").
- The old `local_pid_namespace_proc_isolation` env-readability masking is gone
  (folded into the contrast assertion #7).
- `docs/security.md` / `docs/testing.md` updated to describe the guarantee
  ("isolated targets and their descendants are unreadable by unprivileged
  same-uid callers; root can read; persisted files are the user's") and to point
  at this test as the proof.
- `nix build .#<vm test attr>` green end to end.

## Out of scope

- Per-invocation transient uids (future `2670`); supplementary groups; real
  session D-Bus.
