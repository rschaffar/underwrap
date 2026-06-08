# Task: Comprehensive VM test proving target + descendant isolation end to end

Parent: `2600_story_target_uid_isolation`
Depends on: `2630`, `2640`, `2645` (hard). `2650` (userns relabel) is **WON'T
DO** for the first release (cosmetic; arms the userns LPE class — see its
ticket), so gate #10 asserts the *non-relabel* case: `getuid()==X` and `$HOME`
files display as the dedicated user, with the target still fully functional.

## Status: partially proven inline; this ticket finishes the matrix

Already landed in `nix/tests/test.py` (committed `2a2b57a`/`0834ce9`) and green:
`local_target_uid_isolation` (isolated subtree direct + descendant: environ/maps/
smaps/auxv/mem all denied to the same-UID caller; real uid is a dedicated
`underwrap-target-*` uid, ≠0/≠caller, member of only its own group; secret
unrecoverable), `local_target_uid_idmapped_home` (idmapped dir read-as-owner +
writeback-owned-by-caller), and the corrected unisolated baseline contrast.

Also landed since (commit for 2635): `local_target_uid_files_libsecret`
(isolated **files** write→persist→read round-trip via the setfsuid(X) window;
isolated **libsecret** store/lookup as uid X with backend persistence; caller
cannot see the staging tmpfs or the private bus from the host ns), and
`local_ancestor_attack` was re-pointed at the **isolated** fixture (deterministic
UID boundary denies an ancestor's environ/ptrace/mem; unique `sleep 5252`
marker), replacing its earlier reliance on the incidental, non-deterministic
non-dumpability of an unisolated target.

**Still to add here** (the comprehensive matrix): explicit `process_vm_readv` and
`ptrace(PTRACE_ATTACH)` probes against the isolated target itself (the ancestor
variant now exists; the same-host non-ancestor variant is still TODO); the
**root-can-read** documented-limitation assertion; the **authority** hard checks
from 2620/2630 (getcap shows cap_setuid+cap_setgid; root-owned
`/etc/underwrap/target-uids` is `root:root` and not world-writable; a bogus fd-3
bundle cannot change which uid the target lands on; `ExecBundle` carries no
target-uid field via a protocol round-trip test); and the **2645 boundary
proofs** (TIOCSTI-injection denied, `/proc/<pid>/fd` hygiene, fresh IPC ns,
anonymous session keyring). The files/libsecret-under-isolate functional proofs
(2635) are in; the matrix should fold in the symlink-TOCTOU writeback proof and
the read-only-grant drop under isolate.

## Goal

A single decisive VM-test section that proves the whole property holds together:
a same-UID caller (and processes it spawns) cannot inspect an isolated target or
any of its descendants, root still can, and the target remains fully functional
(home rw, sockets, libsecret, writeback). This replaces the old
`local_pid_namespace_proc_isolation` + `local_ancestor_attack` confidence with a
real matrix, and is the gate before flipping production wrappers (e.g.
`aws-sso`) to `isolate = true`.

## Fixtures (in `nix/tests/node.nix`, as isolated wrappers — `isolate = true`)

- `test-iso-sleep` — `sleep 300`, `env = true`, secret
  `ISO_SECRET=iso_secret_val` in its env. Long-lived target for `/proc` probing.
- `test-iso-tree` — a script that `exec`s a child `sleep 300` (descendant), so we
  can probe the **child's** `/proc`. Secret in env.
- `test-iso-home` / `test-iso-id` — reuse from 2640/2650 for the functional half.
- Keep an **unisolated** `test-sleep` for the contrast assertions.

**VM filesystem caveat:** the idmapped-`$HOME` assertions (#11) require
`testuser`'s `$HOME` to sit on an **idmap-capable** fs (`FS_ALLOW_IDMAP`:
ext4/btrfs/xfs/tmpfs — **not** 9p, which the NixOS test framework uses for the
host store share). Pin the fixture's home onto a supported backing (e.g. an
explicit tmpfs or ext4 image mount for `/home`) so the test is deterministic, and
also add a negative test that an unsupported backing fails *closed* with the
clear 2640 error (not silently inaccessible).

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
7. **Contrast (this is what proves the epic does something):** run the same probes
   against the **unisolated** baseline (per the corrected 2610 baseline — read it).
   The *direct* `test-sleep` target is **already non-dumpable**, so its READ **and**
   ATTACH classes are **all denied even when unisolated** (an incidental side-effect
   of the wrapper chain, not the uid boundary). So the direct target alone cannot
   prove anything. The meaningful difference is the **descendant**: an unisolated
   env-mode target's spawned child has its `environ` **readable** (dumpable,
   caller-uid) with the secret recoverable, whereas the **isolated** descendant's
   `environ` is **denied** (uid boundary). Assert that descendant readable→denied
   transition exactly — it is the core proof — and assert the direct target stays
   denied in both cases, so a regression that silently disables isolation still
   surfaces via the descendant probe. Do **not** re-introduce the false "direct
   target READ-class readable at baseline" claim.

Root-can-still-read (document the boundary, don't pretend otherwise):

8. As **root**, `cat /proc/<pid>/environ` of the isolated target **succeeds**
   (CAP_SYS_PTRACE bypasses the uid gate). Assert success and log it as the
   documented limitation, so the test encodes the true threat model.

Identity + functionality (ties in 2640/2650):

9. Read as root: isolated target's real uid == the configured
   configured `underwrap-target-<n>` uid X — **and never `0`/caller/service id**
   (this doubles as the confused-deputy guard assertion from 2620/2630).
10. Inside the target: `getuid()==geteuid()==X` and `$HOME` files display as the
    dedicated `underwrap-target-<n>` user — confirming the **non-relabel** model
    (2650 is WON'T DO). Assert the target is still fully functional (file access
    works regardless of the displayed identity — proven by the idmapped home).
11. Home rw: target reads a pre-seeded `$HOME` file and writes one; the new file
    is on-disk owned by `testuser`; host `$HOME` ownership/mode unchanged (2640).
12. Socket: target `connect()`s a `testuser`-owned `0700` socket in
    `$XDG_RUNTIME_DIR` (2640).
13. libsecret still works: the existing `test-libsecret` store/lookup scenario
    passes when the wrapper has `isolate = true` (dbus-daemon +
    secretsd run as X on the bus tmpfs handled per 2630's staging-access
    mechanism; private bus still reachable by the target, still invisible to the
    host — re-assert the "no bus socket on host" check, and that the caller cannot
    enter `/tmp/.underwrap-ns-bus` from the host namespace). **Specifically
    exercise that the supervisor can still read `libsecret-state.jcs`** at
    writeback (the cross-uid read path from 2630 — this is the case where caps are
    otherwise dropped early at `main.rs:298`).
14. Files-slot writeback still works: a `files`-slot wrapper on the isolated
    instance persists changes — **including the case where the target *replaces*
    a projected file** (atomic temp+rename, new file owned by X), so the test
    actually exercises the cross-uid read path rather than only in-place edits.
    The uid-X target writes into the staging; the supervisor (still caller-uid)
    reads the changes (per 2630) and completes Checkin to the **backend** (this is
    channel/blob persistence, *not* idmap — no artifact lands on the user's disk).
    Also assert the caller cannot see the staging tmpfs from the host namespace.
    (The idmap-owned-by-caller on-disk artifact is the `$HOME` case — covered by
    #11.)
15. Capabilities: isolated target reports all-zero Eff/Prm/Inh/Amb caps.

Signal semantics (note, light assertion):

16. With real=eff=saved=X, `testuser` cannot `kill` the isolated target
    (`kill_ok_by_cred` requires matching uid/suid) — assert EPERM on `kill` from
    `testuser`, and that **root** (or tty SIGINT) can still terminate it. Keep
    this informational if it proves flaky for interactive vs. plain targets.

fd-reopen vector and world-readable `/proc` scope (precision, not pretense):

17. **fd re-open denied:** as `testuser`, `readlink`/open of the isolated
    target's `/proc/<pid>/fd/*` and `/proc/<pid>/{cwd,root,exe}` returns EPERM
    (ptrace-gated, `fs/proc/base.c:1853→718`) — i.e. the caller cannot re-open the
    target's open secret file or unix socket. This is a *positive* property of the
    boundary; assert it explicitly.
18. **World-readable residue is in-scope-but-secret-free:** as `testuser`,
    `cat /proc/<pid>/{cmdline,status,mounts,mountinfo,cgroup}` of the isolated
    target **succeeds** (these are world-readable regardless of uid) — assert the
    reads succeed **and** that the secret marker `iso_secret_val` appears in
    **none** of them. Encodes the honest scope: the boundary hides secret-bearing
    files, not process metadata; secrets never live in argv/mount-paths.

target → caller escalation closed (the boundary's second direction, 2645):

19. **TIOCSTI:** the isolated target's attempt to inject into the caller's
    controlling terminal returns EPERM/EIO and the injected sentinel never runs in
    the launching shell (per 2645). Contrast against a fixture without `setsid`
    to prove the test would otherwise catch it.
20. **No leaked fds:** the isolated target's `/proc/<pid>/fd` (read as root)
    contains only stdio — no idmap `ns/user` fd, ExecBundle pipe, channel socket,
    or dbus/secretsd fd (per 2645).

## Acceptance

- `local_target_uid_isolation()` is wired into the test run and asserts the full
  matrix above (hard asserts on 1–7, 9–15, 17–20; 8 asserts root-success; 16
  best-effort with clear logging).
- No escape-hatch PASS paths: if a target/descendant can't be located by root,
  the test fails (not "invisible ⇒ pass").
- The old `local_pid_namespace_proc_isolation` env-readability masking is gone
  (folded into the contrast assertion #7).
- `docs/security.md` / `docs/testing.md` updated to describe the guarantee
  ("isolated targets and their descendants are unreadable by unprivileged
  same-uid callers; root can read; persisted files are the user's") and to point
  at this test as the proof. The docs must also state the **target-uid authority
  rule** (mirroring "the daemon, not the wrapper, chooses the target binary"):
  the target uid/gid come from the **root-owned `target-path → uid` table**, read
  directly by `underwrap-ns` and keyed on the daemon-chosen target path; they never
  travel on the wire (`ExecBundle` carries no target id) — so a compromised
  `underwrap-exec` cannot redirect the `cap_setuid` switch to root or another
  service.
- `nix build .#<vm test attr>` green end to end.

## Out of scope

- Per-invocation transient uids (future `2670`); supplementary groups; real
  session D-Bus.
- **Network/session containment of an untrusted target** (X11 keylogging, session
  D-Bus, abstract sockets reachable via the shared net ns). This matrix proves
  *secret confidentiality against same-uid inspection* + the cheap boundary
  hardening (2645); it does **not** prove the target is sandboxed from the user's
  GUI/session, which is an explicit non-goal (2600) requiring net-ns + pasta +
  X11/D-Bus proxying. Do not add assertions implying otherwise — and do not let a
  green run be read as "the target is contained."
