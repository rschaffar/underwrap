# Task: Correct the env-exposure claims and land an honest, non-masking VM baseline

Parent: `2600_story_target_uid_isolation`

## Problem

Several artifacts assert protections that don't exist *and* describe an exposure
that — as we actually build it — isn't where the docs say it is. The VM test is
also structured so it can pass without checking the thing that matters. Three
distinct errors:

1. **`hardenTargetHost` doc + module comments claim `ptrace_scope=2` protects
   `environ`.** It does not. Yama acts only on `PTRACE_MODE_ATTACH`
   (`security/yama/yama_lsm.c`), so `ptrace_scope` gates `mem` /
   `ptrace(ATTACH)` / `process_vm_readv` but **never** the `PTRACE_MODE_READ`
   files `/proc/<pid>/{environ,maps,smaps,auxv}`.

2. **The tickets/docs claim an exec'd env-mode target's `environ` is same-UID
   readable today.** **It is not** (VM-verified, kernel 6.18.20): the wrapper
   cap + namespace chain makes the **direct target non-dumpable** — `environ` is
   `root:root 0400`, and a same-UID caller is denied the whole READ class
   (`environ`/`maps`/`smaps`/`auxv`) **and** the ATTACH class (`mem`). This is an
   **incidental** side-effect (a secure-exec that forces `dumpable =
   suid_dumpable`), not a designed control. The genuine, durable exposure is the
   target's **descendant processes**: a clean child `execve` resets `dumpable` to
   `1`, and the child still runs as the **caller's uid**, so a same-UID attacker
   can read the child's `environ` (which inherited the secret). See 2600 "Why
   this is needed" for the full, kernel-pinned rationale — this ticket exists to
   get that rationale into the docs and the test.

3. **`nix/tests/test.py` `local_pid_namespace_proc_isolation` masks the reality.**
   It (a) asserts the *wrong* baseline (target `environ` readable), (b) lumps the
   READ-mode files with `mem`, and (c) if root's `pgrep` doesn't find the target
   it logs `"PASS: target process invisible from parent namespace"` and never
   reads anything — turning "couldn't find it" into a green check. Root sees all
   PID namespaces; a missing target means the wrapper failed to start.

## Fix

Make the docs accurate and the test honest. This ticket does **not** add the UID
boundary (2620–2650) — it establishes the correct, non-masking baseline so the
later tickets move the test from RED→GREEN for the *right* reason (the descendant
case and the explicit-vs-incidental upgrade).

### Docs / `docs/security.md` — carry the whole rationale (this is the point)

`docs/security.md` (and the `hardenTargetHost` option doc + the module's
`ptrace_scope` comment) MUST state the corrected picture in full, so nobody —
human or agent — re-derives the wrong version. Mirror 2600 "Why this is needed":

- **`ptrace_scope=2` is defense-in-depth for the ATTACH vectors only**
  (`mem`/ptrace/`process_vm_readv`); it never gates the READ-mode files
  `/proc/<pid>/{environ,maps,smaps,auxv}`, which are gated solely by the target's
  *dumpable* flag.
- **The direct env-mode target is already non-dumpable today** through the
  wrapper cap/namespace chain, so its `environ` is denied to same-UID callers —
  but this is an **incidental, fragile, undocumented side-effect**, not a control
  to rely on. State the kernel facts that pin it down:
  - `execve` forces `dumpable = suid_dumpable` (same-UID-denied) iff the binary
    is unreadable **or** `euid != ruid` **or** `egid != rgid`; otherwise it
    resets `dumpable` to `1`. So `PR_SET_DUMPABLE(0)` does **not** survive a clean
    exec.
  - `ptrace_may_access` admits a reader only if its uid equals **all** of the
    target's real/effective/saved uids (or it has `CAP_SYS_PTRACE`).
- **The real exposure is the target's descendants:** a clean child exec resets to
  dumpable and runs as the caller's uid → its `environ` (inherited secret) is
  same-UID readable. Today only the *direct* non-exec'ing wrapper stages
  (`underwrap-exec`, the `underwrap-ns` supervisor, secretsd) are *intentionally*
  protected — by the **dumpable flag** (setgid-induced / explicit
  `PR_SET_DUMPABLE=0`), not by `ptrace_scope`.
- **Why the per-target UID is the fix and dumpable hardening is not:** `dumpable`
  is per-exec and resets; a distinct real uid X covers the whole subtree and
  never resets, and makes the protection explicit/robust. (Forward-reference the
  2600 epic.)
- **Why `cap_setuid` is acceptable** (mirror 2600's subsection): `underwrap-ns`
  already holds `CAP_SYS_ADMIN` in the caller's process under a mandatory
  drop-before-exec discipline; `CAP_SETUID` rides the same discipline; the
  root-owned table + floor check (not a cap limit) defend against `X=0`; a
  per-target setuid-X launcher is the bounded alternative.
- **World-readable `/proc` residue that *no* mechanism here hides** (neither
  `ptrace_scope`, dumpable, nor the 2600 uid boundary): `cmdline`, `comm`,
  `stat`, `status`, `mounts`, `mountinfo`, `cgroup`, `limits`. None carry secret
  values (secrets live in env/files/libsecret, never argv) — a scope statement,
  not a hole. Conversely, the **positive**: `fd`/`fdinfo`/`cwd`/`root`/`exe`
  links **are** ptrace-gated, so a same-UID caller cannot re-open a target's open
  secret file or socket.

### VM test harness — assert the TRUE baseline, by access class and by hop

In `local_pid_namespace_proc_isolation`:

- **Remove the escape hatch.** If root cannot locate the target, that is a HARD
  failure (root sees all PID namespaces). Delete the `"invisible from parent
  namespace" -> PASS` branch.
- **Direct target, today (unisolated):** assert the whole READ class
  (`environ`/`maps`/`smaps`/`auxv`) **AND** the ATTACH class (`mem`) are **denied**
  to a same-UID caller — the target is non-dumpable. (This flips the previous,
  wrong "READ class readable" assertion. Encode it via a
  `_assert_target_proc_access(host_pid, isolated=...)` helper so the isolated
  branch — added by 2630/2660 — asserts the same "all denied" with a *different*
  cause, the uid boundary, including for descendants.)
- **Descendant, today (unisolated) — the residual:** add a fixture whose target
  spawns a **persistent child sub-process** (e.g. a script that backgrounds a
  `sleep` and the test reads the child's `/proc/<child>/environ`). Assert the
  child's `environ` **is readable** by a same-UID caller and **contains the
  injected secret** — this is the exposure the epic closes, and it must be proven
  to exist before 2630 closes it. (If empirically the descendant turns out
  *also* protected, that is a new finding to record, not a silent skip.)
- The non-exec'ing `underwrap-ns` supervisor must stay non-dumpable (it sets
  `PR_SET_DUMPABLE=0` and never re-execs); keep asserting its `environ` is denied
  to same-UID callers.

In `local_ancestor_attack`: the ancestor is same-UID, so for the *unisolated*
baseline it can read a **descendant's** dumpable `environ` — hard-fail only on
`SECRET_LEAKED`; the decisive protected-case matrix moves to 2660.

## Acceptance

- `docs/security.md`, the `hardenTargetHost` option doc, and the module
  `ptrace_scope` comment no longer claim `ptrace_scope` protects `environ`, and
  they carry the full corrected rationale above (direct target already non-dumpable
  but incidentally/fragile; descendants are the real exposure; per-target uid is
  the fix; `cap_setuid` rationale; world-readable residue + fd-link positive).
- The VM test has no code path where "target not found" yields PASS.
- On `main` with no other change the VM test **passes** by asserting the honest
  baseline: **direct target — READ *and* ATTACH classes all denied** (non-dumpable);
  **a descendant — `environ` readable and carrying the secret** (the residual).
- `nix build .#checks.x86_64-linux.vm-test` green; `cargo fmt --check`.

## Out of scope

- Adding the UID boundary (2620–2650) and the full proof matrix (2660). This
  ticket only corrects documentation/claims and de-masks the test onto the true
  baseline.
