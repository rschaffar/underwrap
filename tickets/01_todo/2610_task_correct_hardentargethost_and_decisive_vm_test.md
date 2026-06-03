# Task: Correct the `hardenTargetHost` claim and stop the VM test masking the env exposure

Parent: `2600_story_target_uid_isolation`

## Problem

Two artifacts assert a protection that does not exist, and the test is
structured so it can pass without ever checking the thing that's broken.

1. **`nix/modules/underwrap.nix:457-472` and `:614-621`** — the
   `hardenTargetHost` option doc claims `kernel.yama.ptrace_scope=2` means "only
   processes with CAP_SYS_PTRACE can ptrace or read `/proc/<pid>/{environ,mem}`".
   That is true for `mem` (opened `PTRACE_MODE_ATTACH`) but **false for
   `environ`** (opened `PTRACE_MODE_READ`; Yama only gates `PTRACE_MODE_ATTACH`,
   `security/yama/yama_lsm.c:355`). The same is true for `maps`, `smaps` and
   `auxv` — all `PTRACE_MODE_READ` (`fs/proc/task_mmu.c:352`, `fs/proc/base.c:1078`).
   So `/proc/<pid>/{environ,maps,smaps,auxv}` of a same-UID dumpable target are
   **all** readable regardless of `ptrace_scope`; only `mem`, ptrace-attach and
   `process_vm_readv` are ATTACH-mode and thus gated.

2. **`nix/tests/test.py:728` `local_pid_namespace_proc_isolation`** — the
   "decisive" test asserts the caller cannot read the target's `environ`, but:
   - it lumps the READ-mode files (`environ`/`maps`) together with `mem` and
     even asserts `maps` **denied** (`test.py:837`) — which is a *false*
     assertion that only "passes" because of the escape hatch below; and
   - if root's `pgrep` doesn't locate the target it logs
     `"PASS: target process invisible from parent namespace"` and **never reads
     `environ`** — turning "couldn't find it" into a green check. (The rationale
     in that branch — "PID namespace hides it" — is itself wrong: root sees all
     PID namespaces; a not-found target means the wrapper failed to start.)

3. **`nix/tests/test.py:862` `local_ancestor_attack`** only *hard*-fails on
   `SECRET_LEAKED` and merely *warns* on `ENVIRON_READABLE_NO_SECRET`, even though
   the `test-sleep` fixture injects `DUMPABLE_SECRET` into the target env
   (`:583`) — so a real leak can pass as a warning.

## Fix

Make the docs accurate and the test honest. This ticket does **not** add the new
protection — it establishes the correct, non-masking baseline so the later
tickets have a test that actually moves from RED→GREEN.

### Docs / module

- Rewrite the `hardenTargetHost` description: `ptrace_scope=2` protects only the
  **ATTACH-mode** vectors — `/proc/<pid>/mem`, ptrace-attach, and
  `process_vm_readv` — **not** the **READ-mode** files
  `/proc/<pid>/{environ,maps,smaps,auxv}`, which are gated only by the target's
  dumpable flag. State that protecting a target's *environ* (and `maps`/`auxv`)
  from same-UID callers requires the UID boundary added in story 2600, and that
  ptrace_scope is therefore **defense-in-depth**, not the load-bearing control
  for environ.
- Update `docs/security.md` (and any "strong non-caller-inspection claim"
  wording) to match: today, only the non-exec'ing wrapper stages are protected
  from same-UID `environ` reads, and that protection is the **dumpable flag**
  (setgid-induced for `underwrap-exec`, explicit `PR_SET_DUMPABLE=0` for the
  supervisor and secretsd) — not ptrace_scope. Exec targets are not protected
  until 2630+.
- While correcting the doc, also state the **world-readable `/proc` residue** that
  *no* mechanism here hides (neither ptrace_scope, dumpable, nor the 2600 uid
  boundary): `cmdline`, `comm`, `stat`, `status`, `mounts`, `mountinfo`, `cgroup`,
  `limits` are world-readable (`cmdline` has no ptrace gate — `fs/proc/base.c:393`).
  None carry secret values — secrets live in env/files/libsecret and never in
  argv (existing invariant) — so this is a scope statement, not a hole. Conversely
  note the *positive*: the `fd`/`fdinfo`/`cwd`/`root`/`exe` links **are**
  ptrace-gated, so a same-UID caller cannot re-open a wrapper/target's open secret
  file or socket. This keeps the security doc from over- or under-claiming.

### VM test harness

In `local_pid_namespace_proc_isolation`:
- **Split the assertions by access class**, not by file: the READ-mode class
  (`environ`, `maps`, `smaps`, `auxv`) is asserted separately from the
  ATTACH-mode class (`mem`, ptrace-attach, `process_vm_readv`). Today the READ
  class is **readable** and the ATTACH class is **denied** (by ptrace_scope=2).
  Assert that *current* reality (see below).
- **Remove the escape hatch**: if root cannot locate the target via `pgrep`,
  that is `INCONCLUSIVE`/fail, not PASS. Root sees all PID namespaces; failure to
  find the `sleep` target means the wrapper didn't start, which must fail loudly.
  Delete the `"PASS: target process invisible from parent namespace"` branch.
- Introduce a module-level switch keyed off the new `isolateTargetUid` flag
  (added in 2620) so this one test asserts:
  - `isolateTargetUid = false` (current default): READ class
    (`environ`/`maps`/`smaps`/`auxv`) **readable**, ATTACH class
    (`mem`/`process_vm_readv`/ptrace) **denied** — encode today's true behavior so
    a regression in the ptrace_scope ATTACH protection is still caught. **Note:
    this means the existing `assert rc_maps != 0` at `test.py:837` must flip to
    asserting `maps` is *readable*** — it is, and the current "pass" is an
    artifact of the escape hatch.
  - `isolateTargetUid = true` (after 2630): READ class **and** ATTACH class
    **all denied** (the uid gate). (This branch is exercised by the fixtures
    2630/2660 enable.)

In `local_ancestor_attack`: hard-fail on `ENVIRON_READABLE_NO_SECRET` **when the
fixture is expected to be protected**; keep the current behavior for the
unprotected baseline. (The decisive secret-leak matrix moves to 2660.)

## Acceptance

- `hardenTargetHost` doc and `docs/security.md` no longer claim `ptrace_scope`
  protects `environ` (or `maps`/`auxv`); they name the dumpable/UID-boundary
  reality and describe ptrace_scope as defense-in-depth for the ATTACH vectors.
- The VM test no longer has a code path where "target not found" yields PASS.
- The VM test, on `main` with no other change, **passes** by asserting the honest
  baseline (READ class `environ`/`maps`/`smaps`/`auxv` **readable**, ATTACH class
  `mem`/`process_vm_readv`/ptrace **denied**) — proving the harness now actually
  reads each file and distinguishes the two access classes.
- `nix build .#<the VM test attr>` green; `cargo fmt --check`.

## Out of scope

- Adding the UID boundary (2620–2650) and the full proof matrix (2660). This
  ticket only fixes documentation and de-masks the test.
