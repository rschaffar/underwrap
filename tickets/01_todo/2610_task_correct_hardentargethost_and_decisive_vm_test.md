# Task: Correct the `hardenTargetHost` claim and stop the VM test masking the env exposure

Parent: `2600_story_target_uid_isolation`

## Problem

Two artifacts assert a protection that does not exist, and the test is
structured so it can pass without ever checking the thing that's broken.

1. **`nix/modules/underwrap.nix:457-472` and `:614-621`** — the
   `hardenTargetHost` option doc claims `kernel.yama.ptrace_scope=2` means "only
   processes with CAP_SYS_PTRACE can ptrace or read `/proc/<pid>/{environ,mem}`".
   That is true for `mem`/`maps` (opened `PTRACE_MODE_ATTACH`) but **false for
   `environ`** (opened `PTRACE_MODE_READ`; Yama only gates `PTRACE_MODE_ATTACH`).
   `/proc/<pid>/environ` of a same-UID dumpable target is readable regardless of
   `ptrace_scope`.

2. **`nix/tests/test.py:728` `local_pid_namespace_proc_isolation`** — the
   "decisive" test asserts the caller cannot read the target's `environ`, but:
   - it conflates `environ` with `mem`/`maps` in one block, and
   - if root's `pgrep` doesn't locate the target it logs
     `"PASS: target process invisible from parent namespace"` and **never reads
     `environ`** — turning "couldn't find it" into a green check.

3. **`nix/tests/test.py:862` `local_ancestor_attack`** only *hard*-fails on
   `SECRET_LEAKED` and merely *warns* on `ENVIRON_READABLE_NO_SECRET`, even though
   the `test-sleep` fixture injects `DUMPABLE_SECRET` into the target env
   (`:583`) — so a real leak can pass as a warning.

## Fix

Make the docs accurate and the test honest. This ticket does **not** add the new
protection — it establishes the correct, non-masking baseline so the later
tickets have a test that actually moves from RED→GREEN.

### Docs / module

- Rewrite the `hardenTargetHost` description: `ptrace_scope=2` protects
  `/proc/<pid>/mem`, `/proc/<pid>/maps`, ptrace-attach, and `process_vm_readv`
  (all `PTRACE_MODE_ATTACH`), **not** `/proc/<pid>/environ` (`PTRACE_MODE_READ`,
  gated only by the dumpable flag). State that protecting a target's *environ*
  from same-UID callers requires the UID boundary added in story 2600.
- Update `docs/security.md` (and any "strong non-caller-inspection claim"
  wording) to match: today, only the non-exec'ing wrapper stages are protected
  from same-UID `environ` reads; exec targets are not (until 2630+).

### VM test harness

In `local_pid_namespace_proc_isolation`:
- **Split** the assertions: `environ` (READ-mode) is asserted separately from
  `mem`/`maps` (ATTACH-mode). Today `mem`/`maps` are denied (keep asserting
  that); `environ` is readable (assert the *current* reality — see below).
- **Remove the escape hatch**: if root cannot locate the target via `pgrep`,
  that is `INCONCLUSIVE`/fail, not PASS. Root sees all PID namespaces; failure to
  find the `sleep` target means the wrapper didn't start, which must fail loudly.
- Introduce a module-level switch keyed off the new `isolateTargetUid` flag
  (added in 2620) so this one test asserts:
  - `isolateTargetUid = false` (current default): `environ` **readable**,
    `mem`/`maps` **denied** — i.e. encode today's true behavior so a regression
    in the ATTACH-mode protection is still caught.
  - `isolateTargetUid = true` (after 2630): `environ`, `mem`, `maps` **all
    denied**. (This branch is exercised by the fixtures 2630/2660 enable.)

In `local_ancestor_attack`: hard-fail on `ENVIRON_READABLE_NO_SECRET` **when the
fixture is expected to be protected**; keep the current behavior for the
unprotected baseline. (The decisive secret-leak matrix moves to 2660.)

## Acceptance

- `hardenTargetHost` doc and `docs/security.md` no longer claim `ptrace_scope`
  protects `environ`; they name the dumpable/UID-boundary reality.
- The VM test no longer has a code path where "target not found" yields PASS.
- The VM test, on `main` with no other change, **passes** by asserting the honest
  baseline (environ readable, mem/maps denied) — proving the harness now actually
  reads `environ` and distinguishes the two access classes.
- `nix build .#<the VM test attr>` green; `cargo fmt --check`.

## Out of scope

- Adding the UID boundary (2620–2650) and the full proof matrix (2660). This
  ticket only fixes documentation and de-masks the test.
