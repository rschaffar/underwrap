# Story: Protect each wrapped target's secret from same-UID inspection (per-target uid)

Source: research findings (June 2026) into `/proc/<pid>/environ` exposure of
wrapped targets. The load-bearing conclusions are reproduced in the child tickets;
kernel claims were verified against `~/unsynced_projects/3rdparty/linux` (v7.1-rc6).

## Status & design (read first)

**Design settled (the *simplified* design), not yet implemented.** All child
tickets are in `tickets/0{1,2}_*`; only 2610's doc/test groundwork is written.
An earlier, more complex design (per-instance uid anchored on a kernel "egid
badge") was **abandoned** during implementation — the badge does not survive to
`underwrap-ns` (see "What we abandoned and why" below). This story describes the
design we are actually building.

**Premise correction (2026-06, VM-verified — do NOT re-derive the wrong version).**
The naive motivation — "env-mode leaves the secret in `/proc/<pid>/environ`, so any
same-UID process can read it" — is **false for the direct target as we build it.**
The wrapper chain already makes the target **non-dumpable**, so its `environ` is
**denied** to same-UID callers *today*. The exposure this epic actually closes is
the target's **descendant processes** (which reset to dumpable and run as the
caller's uid), plus converting the fragile, incidental direct-target protection
into an explicit, robust one. The complete, corrected rationale — with the kernel
facts that pin it down — is in "Why this is needed" below. **Read that section
before touching this epic;** getting the premise wrong sent the design in circles
once already.

**The whole design in five sentences:**

1. Each isolated target tool runs under its **own dedicated uid X** (≠ the
   caller), so the kernel's `ptrace_may_access` uid gate denies the caller (a
   *different* uid) from reading the target's `/proc` — `environ` included.
2. *Which* uid is chosen comes from a **root-owned `target-path → uid` table**
   that `underwrap-ns` reads directly; root-owned means a compromised/buggy
   unprivileged stage cannot inject `uid 0` → **no privilege escalation, no
   "confused deputy," no kernel badge, no privilege-drop surgery.**
3. The **exec stays in the wrapper chain** (`underwrap-ns`, running inside the
   user's own process via file capabilities), so the target keeps the user's
   terminal, stdio, cwd, argv, exit status, and signals — none of which a daemon
   re-launching the target could provide for free.
4. The target reads/writes the user's files via **idmapped mounts** that map
   caller-uid ⇄ X inside the target's private **mount namespace** (the one
   namespace this design requires).
5. **Yama / `ptrace_scope` is no longer the load-bearing control for an isolated
   target** (its uid boundary is) — but the module **keeps pinning it**, because it
   still protects *unisolated* targets' `mem`/ptrace from same-UID callers and
   backstops the non-dumpable wrapper stages. It never protected `environ` for
   anyone.

## Why this is needed — the exposure, stated correctly (read carefully)

It is tempting to say "env-mode puts the secret in `/proc/<pid>/environ`, and any
same-UID process can read it." **That is false for the direct target as we
actually build it, and getting it wrong wasted real effort. Here is the accurate
picture — do not re-derive the wrong version.**

**1. The direct target is ALREADY protected — incidentally.** Empirically (NixOS
VM, kernel 6.18.20): an env-mode target exec'd through the wrapper chain comes out
**non-dumpable**. Its `/proc/<pid>/environ` is `root:root 0400`, and a same-UID
caller is **denied** — the whole READ class (`environ`/`maps`/`smaps`/`auxv`) and
the ATTACH class (`mem`). This is a **side-effect** of the cap + namespace chain
(it triggers a secure-exec that forces `dumpable = suid_dumpable`), not a control
we designed and not one fully characterized at the source level. **It must not be
relied upon** — it is fragile and undocumented.

**2. Kernel facts that govern this** (verified against `fs/exec.c:1210-1215`,
v7.1-rc6; reproduced on 6.18):

- On `execve`, the kernel forces `dumpable = suid_dumpable` (0/2 →
  same-UID-denied) **iff** the binary is unreadable by the new creds **OR**
  `euid != ruid` **OR** `egid != rgid`; **otherwise it resets `dumpable` to `1`**
  (same-UID-readable).
- Therefore **`PR_SET_DUMPABLE(0)` does NOT survive a clean `execve`**: a normal
  child exec with matching ids and a readable binary becomes dumpable again.
- `ptrace_may_access` (the gate on `environ`/`mem`/…) admits a reader only if its
  uid equals **all** of the target's real/effective/saved uids (or it holds
  `CAP_SYS_PTRACE`). One mismatched target uid denies the reader for the whole
  `/proc` secret surface.
- Yama / `kernel.yama.ptrace_scope` acts **only** on `PTRACE_MODE_ATTACH`
  (`security/yama/yama_lsm.c`), so it gates `mem`/ptrace/`process_vm_readv` but
  **never** the READ-mode files. (The original `hardenTargetHost` doc claimed
  otherwise; 2610 corrects that.)

**3. The real, durable exposure is the target's DESCENDANTS.** Because a clean
child `execve` resets `dumpable` to `1` *and the child still runs as the caller's
uid*, any subprocess an env-mode target spawns (a shell-out, `git`, a helper) is
**dumpable and caller-UID** → a *different* process running as the same user (the
threat actor) can read **its** `environ`, which inherited the secret. The direct
target is covered by the incidental side-effect; **its children are not.** This is
the exposure the epic actually closes.

**4. Why a per-target real UID is the correct fix — and dumpable hardening is
not.** `dumpable` is **per-exec and resets**; you cannot make it stick across a
process subtree. A distinct real uid X **covers the entire subtree** — children
inherit X, and `ptrace_may_access` denies the caller-UID reader for *every*
process in the tree whether dumpable or not — and it **never resets**. It also
converts the fragile, incidental direct-target protection into an **explicit,
robust, documented** control. That is why we run the target (and therefore all of
its descendants) as X, rather than trying to keep the subtree non-dumpable.

files-mode and libsecret-mode never put the secret in `environ`, so the
descendant-`environ` exposure does not apply to them. **For a multi-process
env-mode target that cannot run under a dedicated uid, prefer files-mode or
libsecret-mode.** This epic's protection is for **env-mode** wrappers, whose
secret — and the same secret inherited into their descendants' env — would
otherwise be same-UID readable.

## The fix — a per-target real UID

`__ptrace_may_access` requires the caller's uid to equal **all three** of the
target's real/effective/saved uid (`kernel/ptrace.c`); a mismatch returns `EPERM`
before the dumpable check or Yama is consulted. So giving the target a **different
real uid X** makes the caller (a different uid) fail the gate for the whole
`/proc` secret surface (READ *and* ATTACH classes) and for the whole subtree
(children inherit X).

**Per target, not per instance, not global.** Each isolated target tool gets its
own dedicated uid. That has a clean consequence the design leans on: a *same-uid*
read of a target's `/proc` can now only come from **that same tool** (another
invocation of it) — the same trust domain — so same-uid reads are harmless by
construction. Wrappers that share the same target binary share its uid (same
tool); finer isolation (per-wrapper, per-invocation) is a future refinement
(`2670`).

### Authority for the uid — root-owned table, read directly (no escalation path)

The uid is the **one** value in the flow that, if attacker-controlled, enables
*privilege escalation* (`target_uid = 0` → a root-owned target). It is therefore
**not relayed and not derived from a forgeable credential.** Instead:

- the NixOS module (the sole root writer of `/etc/underwrap/...`) creates the
  per-target system users and writes a **root-owned table** mapping the target's
  absolute binary path → `uid`/`gid`;
- `underwrap-ns` (the privileged stage) reads that table directly and looks up
  **the target path it is about to exec** (the daemon-chosen path). If the path
  is in the table → switch to its uid; if not → run as the caller (unisolated,
  unchanged behaviour).

Because the table is root-owned, the only uids `underwrap-ns` will ever switch to
are the dedicated sandbox uids the trusted module assigned — `root`, another
service, or another tool's uid are structurally unreachable. Defense-in-depth:
`underwrap-ns` additionally rejects a looked-up `0`/sub-floor uid and fails
closed. This is the analogue of the existing invariant *"the daemon — not the
wrapper — chooses the target binary"*: the privileged stage derives the sensitive
decision from trusted root state, never from the wire. (The target *binary* is
the daemon's choice relayed through the chain; keying the uid on that path inherits
the same, already-accepted, trust level. A compromised unprivileged stage could at
worst (a) run a tool under *another tool's* sandbox uid, or (b) pass a target path
that is *not* in the table so it runs **unisolated** (secret back in a
same-uid-readable env) — but **never** escalate to root. Both are bounded: a
compromised `underwrap-exec` already holds the secret and cannot reach a privileged
uid. Isolation defends the secret against *other same-uid processes*, **not**
against a compromised broker stage. `underwrap-ns` cannot fail-closed on
"not in table" — legitimately-unisolated wrappers hit that path too — so the table
key must be the *exact* configured `target` string the module wrote and the daemon
returns, ensuring a normal isolated wrapper always matches.)

### The exec stays in the wrapper chain (why not the daemon)

Having the privileged **daemon** exec the target would make the target the
daemon's child, with no connection to the user's terminal — breaking stdin/stdout,
Ctrl-C, exit-code propagation, and job control, and turning the always-running
daemon into a privileged "launch anything" service. The two-stage wrapper chain
exists precisely to avoid this: it runs the privilege elevation **inside the
user's own process** (`underwrap-exec` setgid → `underwrap-ns` file-caps), so the
target inherits the user's terminal/stdio/cwd/argv/exit/signals for free, and the
privileged code is short-lived rather than a long-running fork-server. The uid
switch therefore happens in `underwrap-ns`, which already runs in the user's
process and now reads the root-owned table.

### Why `cap_setuid` is acceptable here (and the bounded alternative)

Switching the target to uid X means `underwrap-ns` must `setresuid(X,X,X)`, which
requires `CAP_SETUID` — an **unrestricted** "become any uid, including 0"
primitive. That looks alarming; record why it is acceptable, so it is not
re-litigated every time someone reads the capset:

- `underwrap-ns` **already** holds `CAP_SYS_ADMIN` (for the PID/mount namespaces
  and the idmapped mounts) **in the caller's own process**, and `CAP_SYS_ADMIN`
  is itself root-reachable. So the category *"an exploited `underwrap-ns`, before
  it drops privilege, can reach root"* **already exists**. `CAP_SETUID` does not
  create a new category — it is one more capability under the **same mandatory
  discipline** the chain already depends on: `underwrap-ns` clears **all**
  capabilities (permitted/effective/inheritable/ambient) and sets
  `NO_NEW_PRIVS` before the target runs.
- The root-owned target→uid table constrains **which** uid is chosen; it does
  **not** restrain the capability itself. The defense against `X = 0` is the
  root-owned table plus `underwrap-ns`'s own floor check (reject `0`/sub-floor),
  not any limit on `CAP_SETUID`.
- **Bounded alternative (decided in 2620/2630):** a per-target **setuid-X
  launcher** — a tiny binary owned by the sandbox uid X with the setuid bit —
  switches `euid → X` with **no capability at all**, so even if abused it can
  only become X (a locked-down sandbox uid), never root. Costs: one setuid binary
  per sandbox uid, and a `NO_NEW_PRIVS` ordering wrinkle (the setuid bit is
  ignored once NNP is set, so NNP must be set *after* the switch, inside the
  launcher). 2620/2630 picks between `CAP_SETUID` and the setuid-X launcher and
  records the decision and its reasoning there.

### File access — idmapped mounts (the one namespace we need)

Because the target runs as X, it loses DAC access to the caller's files (owned by
the caller's uid). 2640 restores it with **idmapped bind mounts** of the caller's
`$HOME`/`$XDG_RUNTIME_DIR` that map caller-uid ⇄ X: the target reads/writes the
user's files as the user, and writes land on disk owned by the user. This is the
"portable home directory" idmapped-mount use case. It **requires a private mount
namespace** (an idmapped over-mount of `/home/<user>` must not be visible
host-wide) — that is the single namespace this design depends on. `underwrap-ns`
already creates a mount namespace (for libsecret); the PID namespace it also
creates is kept for cleanup but is not load-bearing for the secret guarantee.

### Yama: kept, but no longer load-bearing for isolated targets

- The **isolated target** is protected by its distinct uid (the gate denies the
  caller for both READ and ATTACH classes before Yama is consulted) — so Yama is
  *redundant* for it.
- An **unisolated target** (running as the caller, dumpable) has **no** uid
  boundary, so a same-UID caller — *including the launching shell* (an ancestor) —
  could read its `mem`/ptrace it; the *only* thing preventing that is
  `kernel.yama.ptrace_scope=2`. Dropping the pin would therefore **regress every
  unisolated target** — including files/libsecret wrappers, whose secrets transit
  memory even though they never enter `environ`. So the pin **stays**.
- The **wrapper stages** (`underwrap-exec`, `underwrap-ns`) run as the *caller's*
  uid, so the uid trick cannot protect them — but they are **non-dumpable**
  (`underwrap-exec` is setgid; `underwrap-ns` gains file caps → secure-exec →
  dumpable=0; both also set `PR_SET_DUMPABLE=0` explicitly). The kernel's dumpable
  check denies *any* same-uid reader — **including the parent shell** — for
  `environ`, `mem`, and ptrace alike, *before* Yama is reached. So Yama is
  redundant for them too.

Net: the guarantee for an *isolated* target rests on the **uid boundary +
non-dumpable**, not Yama; but the `ptrace_scope=2` pin is **retained** as the
ATTACH-mode protection for *unisolated* targets during rollout and as a cheap
backstop. It never protected the READ-mode files (`environ`/`maps`/`auxv`) for
anyone — 2610 corrects that false claim. `hardenTargetHost` also keeps its
**swap-encryption assertion** (a secret in the target's RAM can surface on
unencrypted swap; the uid boundary does not cover that) and the kernel-floor
check.

## Defense-in-depth bonus: shrinking the target → caller direction (2645)

**This is NOT a goal.** The sole goal of this epic is hiding the brokered secret
from the *caller*; protecting the caller *from* the target was never an objective.
Critically, underwrap does not make the target→caller direction worse: an
*unisolated* target runs **as the caller** and already has strictly **more** reach
over the caller (its files, fds, tty, ptrace of its processes) than an isolated
uid-X target does. Per-target isolation only ever *shrinks* the target's reach
over the caller. The cheap hardenings below shrink it a little further — they are
**defense-in-depth by-products, not guarantees**, and must not be presented as the
point of the feature:

- **Controlling-terminal injection (TIOCSTI).** A uid-X target inherits the
  caller's tty and could inject a command into the caller's shell (an unisolated
  target could too). → `setsid()`.
- **Leaked fds (Leaky Vessels, CVE-2024-21626).** A privileged fd (the idmap
  userns fd from 2640, the ExecBundle pipe, helper fds) reachable via
  `/proc/self/fd`. → `close_range` sweep + `O_CLOEXEC`. (Closing underwrap's own
  internal fds before exec is required regardless of isolation.)
- **Shared IPC namespace + session keyring.** A uid-X target sharing the caller's
  session keyring / host SysV IPC. → `CLONE_NEWIPC`(+`CLONE_NEWUTS`) and
  `keyctl(KEYCTL_JOIN_SESSION_KEYRING)`.
- **setuid-bit planting via the idmapped writeback.** → `MOUNT_ATTR_NOSUID|NODEV`
  on the idmapped mounts (2640; also plain mount hygiene).
- **Symlink-TOCTOU on the files-slot writeback walk.** The supervisor reads the
  X-owned staging under a `setfsuid(X)` window — needed for the cross-uid read
  *anyway* — which bounds the read to uid X, so a target-planted symlink can only
  reach content X already controls, never the caller's files; the walk also
  rejects symlinks outright. (An earlier `openat2(RESOLVE_*)` layer was **dropped
  as redundant for the goal** — the setfsuid window already bounds it.)

## What this does NOT defend against (document, don't pretend)

- **root.** `CAP_SYS_PTRACE` bypasses the uid gate. No same-host mechanism hides
  memory from root. This protects against *unprivileged same-uid* code.
- **The target leaking its own secret.** If the tool prints/logs/persists the
  secret, no uid trick helps. Audit env-mode targets; prefer files/libsecret.
- **Secrets the target persists** into the user's home are the user's files and
  remain readable by the user.
- **Coredumps.** `RLIMIT_CORE=0` does not stop a *pipe* `core_pattern`; the dump
  is owned by **uid X** (so still not caller-readable — an improvement over the
  non-isolated case), but the secret transits to the coredump store (root +
  uid-X readable). Operators who care set a non-pipe `core_pattern`.
- **Unencrypted swap.** Covered by the retained `hardenTargetHost` swap assertion.
- **User-private files outside the idmapped paths.** Per-path bridge; a 0600 file
  owned by the caller outside an idmapped path is `EACCES` to X. Add paths via
  `idmapPaths`. Blanket idmapping of `/` is deliberately not done.
- **World-readable `/proc` metadata.** `cmdline`, `comm`, `stat`, `status`,
  `mounts`, `mountinfo`, `cgroup`, `limits` stay world-readable. None carry secret
  values (secrets live in env/files/libsecret, never argv). The boundary *does*
  hide the secret-bearing READ-mode files and the ptrace-gated `fd`/`cwd`/`root`/
  `exe` links (so the caller cannot re-open the target's secret file/socket — a
  positive property).
- **Sandboxing an *untrusted* target (explicit non-goal).** The target shares the
  caller's network + (unless 2645's IPC ns) IPC namespaces; abstract AF_UNIX
  sockets are net-ns-scoped, so a *malicious* target reaches host X11 / session
  D-Bus (and the idmapped `$HOME` hands it `~/.Xauthority`). Closing this needs a
  container runtime (net ns + pasta + nested X/Wayland + dbus proxy) — out of
  scope. **Untrusted target → real container/VM, not a secret broker.**

## What we abandoned and why (so it isn't reconsidered)

- **Per-instance uid anchored on the kernel `egid` "badge."** The design assumed
  `underwrap-ns` runs with `egid` = the instance group. It does **not**:
  `underwrap-exec` drops the setgid instance-group identity (`setresgid(rgid,…)`)
  **before** forking `underwrap-ns`, so `underwrap-ns` runs with the caller's gid.
  Restoring the badge would require relocating the privilege drop and adding a
  fail-closed gid drop inside `underwrap-ns` for every instance — security-critical
  surgery on the wrapper chain. The root-owned-table approach makes the badge
  unnecessary.
- **Relaying `target_uid` via the `ExecBundle` or the fd-20 channel.** Both are
  mediated by `underwrap-exec`; a compromised stage could inject `uid 0`. The
  root-owned table read by `underwrap-ns` removes the relay entirely.
- **Having the daemon exec the target.** Breaks terminal/stdio/signals and bloats
  the privileged daemon (see "why not the daemon" above).
- **Relying on Yama `ptrace_scope` for the *isolated*-target guarantee.** That now
  rests on the uid boundary + non-dumpable. (The pin itself is **kept** — it is the
  ATTACH-mode protection for *unisolated* targets; see "Yama" above. An earlier
  draft proposed removing the pin "for simplicity" — that was wrong, it would
  regress unisolated targets.)

Kernel floor: every syscall used — idmapped mounts/`mount_setattr` (≥5.12),
`openat2(RESOLVE_*)` (≥5.6), `close_range` (≥5.9) — is under the existing ≥6.5
floor. No new minimum.

## Child tickets (intended order)

1. `2610` — correct the false `hardenTargetHost`/`ptrace_scope` claims (it gates
   only ATTACH-mode, never `environ`/READ-mode; **keep** the pin — it's the
   ATTACH-mode protection for unisolated targets and defense-in-depth for isolated
   ones), and de-mask the VM test so it asserts the honest baseline (READ class
   readable, ATTACH class denied today). *(doc/test groundwork largely written.)*
2. `2620` — NixOS module: per-target sandbox users + the **root-owned
   `target-path → uid` table**; grant `underwrap-ns` `cap_setuid`+`cap_setgid`
   (only when some target is isolated). Per-wrapper `isolate` opt-in.
3. `2630` — `underwrap-ns` reads the table (keyed on the target path), switches to
   the dedicated uid before exec. **Protection lands here.** Includes the
   staging-access design for files-slot/libsecret.
4. `2640` — idmapped bind mounts of `$HOME`/`$XDG_RUNTIME_DIR` (caller ⇄ X) so the
   uid-X target reads/writes the user's files as the user.
5. `2645` — close the target → caller escalations the boundary opens (`setsid`,
   `close_range`+`O_CLOEXEC`, `CLONE_NEWIPC`/`UTS`, session-keyring join).
6. *(optional)* `2650` — user namespace relabel so `getuid()==caller` inside
   (cosmetic; adds userns attack surface; default off).
7. `2660` — comprehensive VM-test matrix proving target + descendant isolation,
   root-can-read, home rw, files-slot/libsecret/writeback, and the target→caller
   closures.

## Out of scope / future

- **Per-invocation / per-wrapper transient uids** (`2670`) — finer than per-target.
- **Supplementary-group access** through the idmapped mounts.
- **Full network/session isolation of an untrusted target** (`2690`) — container-
  runtime-sized; different threat model.
