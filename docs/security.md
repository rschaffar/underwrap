# Security

This document explains the security goals behind underwrap, the mechanisms currently used to pursue them, the assumptions those mechanisms rely on, and the main limitations that still matter.

It is intentionally technical and more detailed than the rest of the documentation.

## Security posture and review status

underwrap is aiming at an awkward problem space: running ordinary local CLIs with secrets, while trying to avoid the usual "just export the token and hope for the best" failure modes.

The project makes strong design choices in pursuit of that goal, but it is still early.

Important framing:

- underwrap already appears useful for reducing **accidental** or **casual** secret exposure in many local and agent-facing workflows
- it is also intentionally designed with a more adversarial caller model in mind
- however, deeper review is still welcome and still needed

So the right way to read this document is:

- these are the current security goals
- these are the current mechanisms
- these are the reasons those mechanisms were chosen
- these are the assumptions and limits you should keep in mind

not:

- this project has been formally verified or independently audited end to end

## What underwrap is trying to protect against

underwrap is primarily trying to reduce or block these classes of problems:

1. **Ambient secret exposure**
   - secrets living in the caller's shell environment
   - secrets leaking into dotfiles, helper scripts, or ad-hoc wrappers
   - secrets becoming easy for unrelated child processes to inherit

2. **Same-user secret theft from nearby processes**
   - another process under the same UID inspecting `/proc`
   - ptrace-style access to short-lived wrapper processes
   - easy reads of wrapper or target state from the host namespace

3. **Wrapper impersonation**
   - a process pretending to be a different wrapper in order to read a secret it should not have

4. **Secret persistence outside the intended backend**
   - temp files
   - sidecar files with secret values
   - local caches that silently bypass the backend model

5. **Cross-instance confusion or leakage**
   - one configured instance reaching another instance's resources

6. **Operational drift toward weaker fallbacks**
   - a security mechanism failing and the system quietly switching to a less safe mode

## What underwrap does not protect against

underwrap does **not** solve these problems:

- **root compromise** — `root` can read memory, files, sockets, and process state
- **kernel compromise** — the design depends on Linux kernel behavior
- **malicious or overpowered target tools** — if the target program itself can reveal the secret, underwrap cannot prevent that
- **unreviewed descendant behavior** — if the target or its helpers execute broad or surprising follow-on behavior, underwrap does not sandbox that for you
- **bad wrapper policy design** — if you wrap a broad CLI, you are still giving the caller broad CLI power
- **all possible agent abuse** — underwrap helps with secret delivery; it does not turn arbitrary local execution into a safe sandbox

That distinction matters. underwrap is a secret-delivery and boundary-tightening system, not a general behavioral sandbox.

This is why the recommended pattern for agent-facing use is still to wrap **your own narrow helper**, not a full general-purpose CLI.

## Threat model summary

A reasonable short summary is:

> underwrap is trying to make "use the secret through this intended command" easier than "obtain the raw secret value" for local CLI workflows, especially on NixOS and especially in agent-facing setups.

That is a meaningful security improvement even when it is not absolute.

## Guarantee under the hardened posture

### Preconditions

The guarantee below applies when all of the following hold:

- The target runs on NixOS with
  `services.underwrap.hardenTargetHost = true` (the default). This
  pins `kernel.yama.ptrace_scope = 2`.
- Every configured swap device is encrypted, or the system has no
  swap. The module asserts this at build time when `hardenTargetHost`
  is enabled.
- Wrappers expose narrow helpers only. A narrow helper, in this
  context, is a program that:
  - does not emit the token on `stdout` or `stderr`,
  - does not persist the token to any path readable by the caller,
  - does not accept attacker-controlled flags or configuration that
    cause it to write the token anywhere the caller can read.
- The mTLS client cert and key are mode `0600` or tighter, owned by
  the daemon user, in a `0700`-or-tighter directory. This is not left
  to operator discipline: `underwrap-keygen` writes them `0600`, and
  the daemon refuses to start if a present credential is over-permissive
  or wrongly owned (see "Target-host daemon hardening").

The "narrow helper" precondition is the single most load-bearing element
of the env-mode guarantee, and deserves more weight than its place in
the list suggests. Yama and the cleared host environment protect the
*target's environ* from same-UID observers; **nothing in the design
protects the secret from the target itself.** A helper that prints,
logs, or persists the token defeats env-mode confidentiality regardless
of how well the kernel isolates it. Use libsecret or files mode wherever
the target supports it; reserve env mode for helpers you have audited
against the three sub-bullets above.

### Guarantee

Under those preconditions, there is no known kernel-mediated path by
which a non-root local attacker — including the caller itself — can
read the raw token value on the target host.

### Residual risk

- **Implementation defects** in the kernel, in the TLS or SSH
  libraries, or in underwrap itself. Absence of a known path is not a
  proof of absence.
- **Root compromise on the target host.** Standard exclusion; `root`
  can read process memory directly.

### Operational trade-off

Pinning `kernel.yama.ptrace_scope = 2` disables unprivileged use of
`strace`, `gdb`, and `perf` against user processes. Deployments that
require these tools for normal operation can lower
`kernel.yama.ptrace_scope` explicitly; the module sets the value with
`mkDefault`, so an overriding entry in
`boot.kernel.sysctl` takes precedence. Lowering the value weakens the
guarantee above to "non-ancestor same-UID processes cannot read the
wrapper or target state; an ancestor process (the caller) can."

---

## Design invariants

These invariants are the backbone of the current design. New features should not quietly violate them.

### 1. All secret material flows through the central backend's `StorageBackend`

Every byte of secret material — whether it is:

- a plain injected token
- mutable credential state
- libsecret-managed data
- directory-credential file contents
- derived access-token source material

is persisted only in `underwrap-backendd`'s `StorageBackend` (`PlainBackend`
or `EncryptedBackend`). The daemon, the wrapper chain, and the namespace
never write secret material to disk: the daemon caches the backend's
opaque resource blob in process memory for the lifetime of the wrapper
channel, the wrapper hands the blob bytes through fd 3 / `SCM_RIGHTS` to
the namespace, and writebacks travel back to the backend over mTLS.

That means:

- no ad-hoc JSON files containing secret values
- no temp files holding secret values
- no sidecar caches on the target host
- no alternate persistence path that bypasses backend policy

Why this matters:

- the backend is where encryption-at-rest decisions live
- the backend is where remote-storage behavior lives
- the backend is where the storage model stays auditable and conceptually centralized

A feature that bypasses the backend's `StorageBackend` is not a convenience
shortcut; it changes the security model.

### 2. The kernel gates access; the daemon gates which wrapper

Two separate questions live here, and underwrap answers them with two
separate mechanisms.

**Can this process talk to the daemon at all?** That is the kernel's
call. The daemon serves one Unix socket per instance, mode `0660`,
owned by the instance group `underwrap-<instance>`. Opening it requires
holding that group as effective GID, and the only way to acquire it is
to exec the root-installed setgid entry point `uw-<instance>` (via NixOS
`security.wrappers`). A process that did not go through the blessed
binary is refused at `connect()`, before the daemon reads a byte. The
daemon then confirms the gate by reading the peer's effective GID —
race-free via `SO_PEERPIDFD` (Linux 6.5+), falling back to
`SO_PEERCRED` + `/proc/<pid>/status` on older kernels — and checking it
equals the instance group.

**Which wrapper does it want?** That is application data: the wrapper
name travels in the request, chosen by the caller. The daemon treats it
as a *selector*, not as proof. It validates the name against the
instance config (rejecting unknown names and the reserved `debug`
token) and then **chooses the target binary itself** — the configured
target for a `Real` invocation, the debug tool for a `Debug`
invocation, and only after the backend's policy has authorized that
`(wrapper, resource, variant)`. The caller never names a target.

The wrapper binaries are world-executable; any local user can invoke
any wrapper. Access control lives in the policy layer, not the
filesystem. The kernel gate (instance group egid) plus the daemon's
name validation plus per-instance backend permissions rule out:

- connecting without the instance group egid (kernel rejects it at
  `connect()`),
- selecting a wrapper the config does not declare (daemon rejects it),
- running the debug tool without an active `UnsafeDebugUntil`
  permission (the daemon picks the target only after the backend
  allows the `Debug` variant — a forged `variant = Real` cannot reach
  the debug tool),
- forging identity via a symlink's `argv[0]` (the daemon never
  consults `argv[0]`; selection is the validated name argument).

### 3. Channels carrying secrets must be authenticated at creation time

Any channel that carries secret material or long-lived authenticated capability must be born from an already authenticated context.

In practice this means:

- the wrapper-socket request is authenticated by wrapper identity
- the exec→namespace handoff uses an inherited anonymous pipe
- the libsecret channel is created during an authenticated wrapper request and then inherited forward
- directory credential channels are created during an authenticated wrapper request and then inherited forward

No user-controlled token or environment-variable based side channel is trusted as the primary authorization mechanism.

### 4. File descriptors carrying secret material must not leak across trust boundaries

The current design relies heavily on anonymous, inherited file descriptors. That is good for avoiding filesystem-visible artifacts, but it only works if those fds are closed before the wrong process can inherit them.

In particular:

- the fd 3 pipe is consumed before the target is exec'd
- the libsecret channel fd must not be inherited by the final target process
- directory credential channel fds must not be inherited by the final target process

### 5. Processes that hold secret material should be non-dumpable whenever possible

If a process is holding secret material in memory and does not need to exec a normal target binary afterward, it should call:

```text
prctl(PR_SET_DUMPABLE, 0)
```

This is central to protecting the wrapper processes themselves.

### 6. No silent fallback to a weaker mode

If a security mechanism fails, the wrapper should abort rather than continue in a degraded mode.

This principle matters most in:

- namespace setup
- mount isolation for libsecret mode
- privilege dropping
- descriptor handling

### 7. The runtime wire never names a storage key

The runtime mTLS API between `underwrapd` and `underwrap-backendd`
speaks **only** `(wrapper_name, resource_name)`. There is no
`secret_id` field, no `prefix` field, no path. The three endpoints
are `POST /v1/wrappers`, `POST /v1/wrapper-invocations`, and
`POST /v1/wrapper-invocations/finalize`. A runtime client cannot
address `__underwrap/policy/document`, `__underwrap/resources/document`,
or any other backend-internal key, because no wire shape exists that
would route a storage key to a write path. The earlier per-key
`/v1/{read,write,list,delete}` surface was deleted in the runtime-API
redesign for exactly this reason: a misbehaving runtime client used
to be able to `POST /v1/write` against `__underwrap/policy/document`
with a hand-crafted policy doc and grant itself everything.

The env section of a resource blob is admin-only on writeback. The
backend enforces `new_blob.env == current_blob.env` on every
finalize and rejects the call with
`InvalidBlob { reason: "env-section-immutable" }` otherwise. Operators
who need to change an env binding go through the admin API.

### 8. The backend is the sole write-authorization authority

The namespace-side tmpfs staging dirs and bind mounts are always
read-write to the target, even when the grant is read-only. This
deliberately diverges from the previous "bind read-only into the
namespace" design, which surfaced EROFS to target processes and broke
tools that test-write before falling back (cache directories, git
credential helpers, etc.).

The backend remains the only authority that decides whether a change
persists. The `writable` flag is re-checked on every
`POST /v1/wrapper-invocations/finalize` independent of the open's
outcome, so a permission flipped from writable to read-only between
open and finalize is honoured. When a wrapper attempts a writeback
for a read-only permission:

- the backend answers `Deny { reason: "not-writable" }`,
- the wrapper drops the locally-buffered change,
- the wrapper emits one structured `AUDIT=writeback_rejected
  REASON=grant_readonly` event directly to systemd-journald via its
  native socket (`/run/systemd/journal/socket`), with
  `RESOURCE_ID` and `RESOURCE_NAME` fields naming the affected
  resource. The audit event is unconditional — it does not pass
  through the wrapper's stderr, so an interactive caller cannot
  observe (or suppress) it,
- and writes one human-readable warning line to stderr — naming the
  resource by both its operator-facing name and its stable id —
  unless `UNDERWRAP_SUPPRESS_READONLY_WARNING=1` is set on the
  wrapper's environment. The suppression env var is never propagated
  into the target process; it is a wrapper-owned signal only.

If journald's native socket is unreachable (non-systemd hosts,
integration test environments), the wrapper falls back to writing the
audit record as one JSON line on stderr. The fallback is *not* gated
by the suppression env var: the audit trail must exist somewhere, and
losing it silently would be worse than the extra terminal noise.

A malicious target inside the namespace can attempt writes against
its bound directory regardless; the backend rejects unauthorized ones
at the IPC layer and leaves an audit trail.

---

## Wrapper identity: one setgid binary, one instance gate

### The shape

Each instance installs exactly one setgid entry point and serves one
socket:

- **binary:** `uw-<instance>` (e.g. `uw-work`), installed via NixOS
  `security.wrappers`, setgid to the instance group
  `underwrap-<instance>`;
- **socket:** `/run/underwrap-<instance>/wrapper.sock`, mode `0660`,
  owned by that same group;
- **group:** `underwrap-<instance>`, which is also the daemon's primary
  group.

Invocation selects the wrapper by argument, not by binary name:

```text
uw-<instance> <wrapper-name> [args...]        # run the configured target
uw-<instance> debug <wrapper-name> [args...]  # run the debug tool
```

The NixOS module additionally installs per-user PATH shims (`<wrapper>`
and `debug-<wrapper>`) that simply `exec` into the forms above. The
shims are plain, unprivileged scripts — no setgid bit, no capabilities;
all privilege lives in `uw-<instance>` and the daemon.

### What the kernel enforces

To open the `0660` socket a process must hold `underwrap-<instance>` as
its effective GID, and that egid is only obtainable by exec'ing the
root-installed setgid `uw-<instance>` binary. So raw access to the
daemon — and therefore to any secret — still requires going through the
blessed binary; a same-UID process that does not is refused at
`connect()`.

### What the daemon enforces

The wrapper name is client-supplied, so the daemon treats it as a
selector and:

- rejects the reserved token `debug` as a wrapper name,
- rejects any name the instance config does not declare,
- **chooses the target binary** — the configured `target` for `Real`,
  `debug_tool_target` for `Debug` — and only emits it to stage 2 after
  the backend has authorized that `(wrapper, resource, variant)`.

Because the *daemon*, not the caller, selects the target, a forged
`variant = Real` can never cause the debug tool to run, and a caller
cannot point a wrapper at an arbitrary binary.

### Why effective GID instead of real GID?

`SO_PEERCRED` reports the *real* GID; a setgid transition shows up in
the *effective* GID, which is the one the daemon needs.

On Linux 6.5+ the daemon obtains a `pidfd` for the peer via
`SO_PEERPIDFD` and reads `/proc/<pid>/status` **while holding that pidfd
open**, so the kernel cannot recycle the PID between the credential
check and the `/proc` read — the lookup is race-free. On older kernels
it falls back to `SO_PEERCRED` (peer PID) plus a `/proc` read,
cross-checking the `SO_PEERCRED` uid/gid against `/proc` to catch PID
reuse, and logs once that the weaker path is in use. Under the hardened
posture (`hardenTargetHost = true`) the NixOS module asserts kernel
≥ 6.5 so the race-free path is always used.

On the `SO_PEERPIDFD` path the daemon resolves the peer PID by parsing
the `Pid:` line of `/proc/self/fdinfo/<pidfd>`. An unexpected procfs
shape — a missing or unparseable `Pid:` line — is treated as a **hard
auth failure by design**: the connection is rejected rather than
silently dropping to the racy `SO_PEERCRED` fallback. This cannot occur
on a supported host, since the `hardenTargetHost` kernel floor (≥ 6.5)
guarantees pidfd fdinfo exposes `Pid:`; the guard exists only so a
hypothetical procfs quirk fails closed instead of weakening the gate.

### Why not trust `argv[0]`?

`argv[0]` is caller-controlled — a symlink can set it to anything — so
the daemon never consults it. Identity is the instance group egid
(kernel-enforced); wrapper selection is the validated name argument
(daemon-enforced).

### Why the NixOS module is load-bearing

The design relies on `security.wrappers` to install the setgid entry
point with the correct ownership, mode, and capability environment.
Doing the same on an ad-hoc system would mean reimplementing what the
NixOS module already does correctly. That is why NixOS is the
supported target platform.

---

## Two-stage wrapper design

### Why split the wrapper?

underwrap needs two things from its wrapper chain:

1. a setgid identity hop
2. the ability to create a PID namespace

On NixOS, those concerns are easier to express as two stages:

- **`underwrap-exec`**: setgid entry wrapper
- **`underwrap-ns`**: capability-bearing namespace helper

### Stage 1: `underwrap-exec` (installed as `uw-<instance>`)

Responsibilities:

- parse argv — an optional leading `debug` token, then the wrapper
  name, then the target's own args
- load the minimal instance sidecar (socket + helper binary paths)
- connect to the instance socket (the setgid instance-group egid is the
  kernel gate)
- send `Read { wrapper_name, variant }`; the daemon validates the name,
  consults the backend policy, and returns the **daemon-chosen** target
  path and environment plus the secret material
- drop elevated privilege
- hand the secret, target, and env to stage 2 over fd 3

### Stage 2: `underwrap-ns`

Responsibilities:

- read the secret and the daemon-chosen target/env from fd 3
- create the namespace environment
- set up the target process environment
- apply final process hardening steps
- exec the target the daemon chose

### Why fd 3?

The secret is passed through an anonymous pipe on file descriptor 3.

This is intentionally not:

- an environment variable between wrapper stages
- a command-line argument
- a temp file

Using fd 3 keeps stdin/stdout/stderr untouched and avoids a filesystem-visible or process-list-visible handoff path.

---

## Privilege drop

`underwrap-exec` drops elevated privilege before the target is reached.

The key steps are:

1. `setgroups(0, NULL)`
2. `setresgid(real_gid, real_gid, real_gid)`
3. `setresuid(real_uid, real_uid, real_uid)`

Using the `setres*` calls makes the permanent drop explicit for the real,
effective, and saved IDs.

That matters because underwrap wants a **one-way** drop here, not a temporary
one. Clearing the saved IDs as part of the drop means the wrapper cannot later
regain the elevated setgid-derived identity through saved credentials.

Each return value is checked. A failed privilege drop is a hard failure.

This follows the project's general rule that silently continuing in a weaker or riskier state is worse than aborting.

---

## Protecting the wrapper processes themselves

The wrapper parent processes are among the most sensitive moments in the whole flow. They briefly hold raw secret material in memory before the target is launched.

### The current mechanism

Both `underwrap-exec` and `underwrap-ns` call:

```text
prctl(PR_SET_DUMPABLE, 0)
```

once they hold secret material.

Because those parent processes do not exec another normal target afterward, the dumpable flag remains in effect for the rest of their lifetime.

### Why this matters

For same-UID processes without `CAP_SYS_PTRACE`, setting a process non-dumpable blocks important inspection paths such as:

- `/proc/<pid>/mem`
- `/proc/<pid>/environ`
- ptrace attach
- related `/proc` access checks that rely on `ptrace_may_access()`

### Zeroization

The wrapper processes also zeroize secret buffers after handoff where practical.

That does not make memory disclosure impossible in every imaginable scenario, but it reduces the lifetime of secret material in process memory.

---

## Protecting the final target process

This is the part most likely to be misunderstood.

### The awkward fact

A normal target binary does **not** stay non-dumpable just because the parent set `PR_SET_DUMPABLE=0` before exec.

For an ordinary exec without a continuing credential mismatch, Linux resets the dumpable state.

So the final target process needs protection from a different combination of mechanisms.

### What currently helps protect the target

Three mechanisms together cover the same-UID inspection surface.

#### 1. Yama LSM, pinned to `ptrace_scope=2`

`services.underwrap.hardenTargetHost` defaults to `true`. The module
sets `kernel.yama.ptrace_scope=2` on the target host. At that level,
`ptrace_may_access()` rejects every tracer that does not hold
`CAP_SYS_PTRACE`, which in turn blocks every same-UID access path
that goes through that check:

- `/proc/<pid>/environ`
- `/proc/<pid>/mem`
- `/proc/<pid>/fd/*` content
- `ptrace(PTRACE_ATTACH)`, `strace`, `gdb`, `perf`

Under `scope=2` the check does not distinguish between an ancestor of
the target and an unrelated same-UID process. Neither holds
`CAP_SYS_PTRACE` and neither passes. This is the difference from the
distribution default `scope=1`, where ancestors are exempted and a
caller process can therefore read `/proc/<pid>/environ` on its own
wrapper descendants.

Unprivileged use of `strace`, `gdb`, and `perf` against user
processes is disabled by `scope=2`. Deployments that require these
tools can lower the value explicitly; the sysctl is declared with
`mkDefault`, so an overriding entry takes precedence. With
`scope=1`, the protection narrows to non-ancestor same-UID processes;
a caller process remains able to read its descendant target's
environment.

#### 2. Credential transitions in the wrapper chain

The target descends from a process tree that included:

- an exec of a setgid binary (`underwrap-exec`)
- an exec of a capability-bearing binary (`underwrap-ns`)

Those privilege-affecting exec transitions matter for ptrace
eligibility and help prevent straightforward tracing from the
original ancestor process even when scope is lowered.

`underwrap-ns` also carries `cap_setpcap` and prunes the capability
bounding set before the target execs. It sets `PR_SET_NO_NEW_PRIVS=1`
before the final target exec so the target and its descendants cannot
gain privilege later through setuid/setgid binaries or file
capabilities. The target should therefore run with zero effective,
permitted, inheritable, and ambient capabilities and cannot acquire
new ones on later execs even if the kernel refuses bounding-set
pruning.

#### 3. Caller-controlled environment is cleared

The target is launched with `env_clear()` and only the keys allowed by
the wrapper config are reintroduced. underwrap also forces the most
security-sensitive location variables instead of inheriting them from
the caller:

- `HOME` comes from the trusted passwd entry (or an explicit trusted
  `envSet.HOME` override)
- `XDG_RUNTIME_DIR` comes from `<xdgRuntimeDirBase>/<uid>`
- `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, and
  `XDG_STATE_HOME` are derived from the effective `HOME` unless
  explicitly overridden via `envSet`

The NixOS module additionally asserts at build time that no wrapper's
`envPassthrough` or `envSet` contains an `LD_*` variable, that
`envPassthrough` cannot name the forced `HOME` / `XDG_*` variables,
and that `envSet` cannot set `XDG_RUNTIME_DIR`. This prevents an
operator from accidentally re-introducing dynamic-linker redirection or
caller-controlled config/runtime-dir steering.

### What the PID namespace contributes

The PID namespace is still useful, but it is **defense in depth**, not
the sole protection for target secrecy. It contributes the same thing in
**every** mode (the `/proc` remount is unconditional now, not
libsecret-only):

- a separate PID space, so the target and its children are isolated and
  the namespace's PID 1 dying makes the kernel SIGKILL the rest;
- a fresh `/proc` mount that reflects only the namespace, so process
  discovery from inside is limited to the namespace's own processes;
- mount isolation where applicable (libsecret mode).

What the PID namespace does **not** give you is confidentiality of the
target's `/proc/<pid>/environ` against a same-UID observer *outside* the
namespace — the namespace contributes containment, not env
confidentiality. That confidentiality comes from Yama
`ptrace_scope = 2` (see below), not from the namespace.

### Important caveats

The secret sits in the target's environ, in cleartext, for the
target's lifetime. Env-injection requires this. The guarantee stated
in [Guarantee under the hardened posture](#guarantee-under-the-hardened-posture)
therefore rests on three preconditions:

1. **`ptrace_scope=2`.** Pinned by the module.
2. **Encrypted or absent swap.** Asserted by the module when
   `hardenTargetHost = true` so that a token paged out of the
   target's memory cannot surface as plaintext on disk.
3. **Narrow wrappers.** The wrapper author is responsible for
   ensuring the target does not print, log, persist, or otherwise
   exfiltrate the token. Where that responsibility cannot be
   discharged for a given target tool, use libsecret or
   directory-credential mode so the raw value never enters the
   target's environ.

Loosening any of these preconditions narrows the guarantee
correspondingly.

---

## Why not user namespaces?

A natural question is: why not use unprivileged user namespaces and avoid `cap_sys_admin`?

The main issue is that the current identity model depends on a setgid transition, and that does not compose cleanly with the required `uid_map` writes for this design.

In other words, the setgid-based identity mechanism and the most obvious user-namespace approach pull in different directions.

---

## Core dump prevention

Before the final target execs, underwrap sets:

```text
RLIMIT_CORE = 0
```

This is meant to prevent a crashing target from leaving secret-bearing core files behind.

This is a simple mechanism, but it matters. Secret-handling systems should be opinionated about core dumps.

---

## Configuration trust and the daemon-chosen target

`uw-<instance>` does not let the caller choose the target binary, and it
never reads the target path from a caller-reachable file. Two configs
are involved, both at fixed paths the NixOS module writes:

- the **daemon config**, `/etc/underwrap/daemon/<instance>.yaml`, holds
  each wrapper's target mapping and the backend connection. Only the
  daemon (and `underwrap-keygen`, for the `backend` section) read it.
- the **frontend-exec sidecar**, `/etc/underwrap/instances/<instance>.yaml`,
  is deliberately minimal: the socket path plus the helper-binary paths
  (`underwrap-ns`, the dbus/secretsd binaries). It carries no target
  paths.

So `uw-<instance>` learns which binary to exec only from the daemon's
reply — after the daemon validated the client-supplied wrapper name
against config and the backend authorized the variant. Every target
path in the daemon config is absolute. A caller cannot redirect the
wrapper to an alternate target through `argv`, environment, or path
tricks.

The daemon-chosen target is especially important for agent-facing
setups, where the distinction between "run this approved helper" and
"run a caller-chosen binary with my secret" is the entire point.

---

## Environment sanitization

underwrap starts the target from a cleared environment and then reconstructs only a limited, trusted set of environment variables.

### Why this matters

A full inherited caller environment is too powerful.

It enables or complicates things like:

- `LD_PRELOAD`
- `LD_LIBRARY_PATH`
- user-supplied `UNDERWRAP_*` control variables
- arbitrary ambient behavior changes in the target

### The model

- a safe default whitelist is retained
- `HOME`, `XDG_RUNTIME_DIR`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`,
  `XDG_CACHE_HOME`, and `XDG_STATE_HOME` are **not** inherited from
  the caller; underwrap forces them to trusted values
- `PATH` is intentionally **not** inherited from the caller
- if a wrapper needs `PATH`, it must be set explicitly from trusted configuration
- helper-selection variables such as `EDITOR`, `VISUAL`, `PAGER`, and `BROWSER` should also be treated as risky and preferably set from trusted configuration rather than caller passthrough
- extra passthrough variables may be allowed per wrapper
- fixed variables may be set per wrapper from trusted configuration; for the forced home/XDG set, use `envSet` only for `HOME` and the base `XDG_*` dirs (with absolute paths), and use the instance-level `xdgRuntimeDirBase` knob for `XDG_RUNTIME_DIR`

This is one of the least glamorous but most important parts of the design. A lot of real-world abuse happens through ambient environment control.

### Default passthrough trade-offs

The default passthrough list (`crates/frontend-ns/src/env.rs`) keeps a
handful of caller-controlled variables for usability. None is wrong by
default, but each is *caller-chosen* and inherited by the **target**, so
they constrain what "narrow helper" can promise. Operators should trim
the list when a given wrapper does not need them:

- **`TMPDIR`** — the caller chooses where the target writes transient
  files, including anything the target derives from the token. Narrow
  helpers should treat `$TMPDIR` as caller-owned and never rely on it
  for secret persistence.
- **`SSH_AUTH_SOCK`** — the target inherits the caller's ssh-agent and
  every key currently loaded into it. A helper that signs over agent
  keys is signing with whatever the caller has loaded, not with a
  wrapper-scoped agent.
- **`DISPLAY` / `WAYLAND_DISPLAY`** — the target inherits the caller's
  display socket (the standard X/Wayland trade-off).

If your wrapper's target does not need one of these, drop it from the
passthrough set so the caller cannot use it as a steering or
exfiltration channel.

---

## Socket permissions and local boundaries

The generated sockets intentionally have different roles.

### The instance wrapper socket

Each instance has exactly one wrapper socket,
`/run/underwrap-<instance>/wrapper.sock`, in the instance runtime
directory.

It is not world-connectable: mode `0660`, owned by the instance group
`underwrap-<instance>`. Local code must hold that group as its effective
GID before it can even `connect()`, and the only way to get it is to
exec the setgid `uw-<instance>` binary. The daemon then re-confirms the
peer's effective GID (race-free via `SO_PEERPIDFD`) before reading the
first frame.

This is a deliberate layering:

- kernel-level permission gating before connection (instance-group
  egid),
- daemon-side validation after connection — the wrapper name in the
  request is checked against config, and the daemon, not the caller,
  picks the target binary.

### Backend-managed administration

The admin surface is not a Unix socket on the target host. Operators
manage wrapper permissions through the central `underwrap-backendd`'s
admin HTTP API (session-cookie authenticated, with synchronizer-token
CSRF) or through the browser-based admin UI in front of it. The target
host only needs outbound mTLS access to the backend.

---

## Mutation gating

There is no global write-lock. The earlier per-store `writes_enabled`
toggle was removed: it conflated "an operator is editing a secret" with
"wrappers may write back", and those now have separate, narrower
mechanisms.

- **Admin secret edits** go through the RESTful per-secret endpoint
  (`PUT /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}`) and are
  unconditional for an authenticated admin — there is no toggle to flip
  first.
- **Wrapper-side writes** (libsecret `SetSecret`, files writeback) are
  gated per permission by the `writable: bool` flag, re-checked
  server-side on every `finalize` (see
  [the backend is the sole write-authorization authority](#8-the-backend-is-the-sole-write-authorization-authority)).
  A read-only permission projects the mutable slot read-only at the
  wrapper edge and the backend rejects any writeback with
  `Deny { reason: "not-writable" }`.

### Trust model implication

Mutation immutability is an **operator-discipline property**, not a
schema invariant: an admin with access can always rewrite a secret, and
any `writable` permission can drive a wrapper-side write. If a future
requirement calls for true tamper-evidence ("this value was never
overwritten"), it has to come from a different mechanism — append-only
storage, audit attestations — not from the policy layer, since policy
itself is operator-mutable.

### Reserved keys

The admin secret endpoints refuse to address the backend-internal
`__underwrap/*` documents (policy, resources, pairs); those are edited
only through their dedicated admin endpoints. The runtime wire has no
shape that can name them at all (see invariant 7).

---

## Structured security logging

The daemon emits structured log events for security-relevant operations such as:

- granted and denied reads (including the `not-unsafe-debug` and
  `expired` deny reasons)
- stores and denied stores
- authentication denials at the socket layer (with the peer-auth path:
  `peerpidfd` or `peercred-fallback`)
- libsecret channel reads, writes, and deletes

The design goal is:

- log the action and relevant identity context
- never log secret material

Logging does not make a system secure, but in systems that are trying to be security-aware, absence of usable security logs is a real weakness. underwrap relies on journald/stdout logging here, not on a separate persisted audit-history product feature.

---

## Central backend security model

Running `underwrap-backendd` centrally changes **where** secrets live,
but not the basic wrapper flow on the target host.

### What the central backend is trying to buy you

- The target host does not need to store any secret material on disk.
- High-value secret material stays on a more trusted workstation or
  server. A compromised target host gets the secret it needed for one
  invocation, not the store.
- Central policy: wrapper modes, rules, and grants are configured in
  one place and enforced by one process. Operators can revoke access
  centrally without touching the target host.

### What it does not buy you

- The target process still receives the credential material it needs
  for the invocation.
- `root` on the target or backend host is still `root`.
- A compromised backend host is still a compromised backend host.

### Target-host daemon hardening

The frontend daemon (`underwrapd-<instance>`) is the only target-host
process that holds the runtime mTLS client cert+key, every live
wrapper-resource channel, and the cached resource blobs for open
invocations. Compromising it is compromising the runtime client identity
— bounded by the backend's per-`(instance, wrapper, resource)` policy,
which still applies, but that is a policy constraint, not isolation. Two
mechanisms tighten it:

- **systemd sandbox.** The unit runs with `NoNewPrivileges`,
  `ProtectSystem=strict`, an empty `CapabilityBoundingSet` /
  `AmbientCapabilities` (the daemon needs no capabilities),
  `MemoryDenyWriteExecute`, `RestrictNamespaces`, `ProcSubset=pid`, and a
  `@system-service` syscall filter minus `@privileged` (with `@chown`
  re-allowed so the daemon can group-own its socket). `/etc/underwrap`
  and the state dir are read-only; `ReadWritePaths` is limited to the
  runtime dir. `ProtectProc` is deliberately left at default — the
  daemon must read the *connecting* peer's `/proc/<pid>/status` for the
  egid check, and that peer runs as a different user, so
  `ProtectProc=invisible` would break authentication.
- **Credential mode check at startup.** `underwrap-keygen` writes the
  cert and key `0600`. The daemon `stat`s them on boot and refuses to
  start if either is wider than `0600`, owned by another uid, or sits in
  a directory wider than `0700`. A key-leak misconfiguration fails loud
  rather than relying on operator discipline.

### Backend trust model

`underwrap-backendd` authenticates two distinct surfaces:

- **Runtime** (`/v1/*`) — mutual TLS. Every `underwrapd` instance gets
  one client certificate, generated locally as a **self-signed leaf** by
  `underwrap-keygen` on the target host. The key never leaves that host;
  an admin pastes the resulting cert PEM into the admin UI's
  **Create client instance** form to register it. The backend
  authenticates by SHA-256 fingerprint match against its runtime-client
  registry: a custom rustls `ClientCertVerifier` accepts any presented
  leaf at the TLS layer (still verifying the handshake signature against
  the leaf's public key, so a stolen cert without its private key cannot
  be replayed), and `resolve_runtime_client` then rejects any cert whose
  fingerprint is not registered as `Active`. There is no CA chain
  validation, no subject/SAN/EKU consultation, and no enrollment
  signing key on the backend.
- **Admin** (`/v1/admin/*`) — same HTTPS listener, no client certificate.
  Operators authenticate with username + Argon2id password hash. Sessions
  use [`tower-sessions`] with an in-memory store: the `__Host-uw-session`
  cookie carries only an opaque session id, the server-side store
  carries the operator id and a copy of the operator's
  password-hash bytes via `axum-login`'s `session_auth_hash()`
  mechanism. Rotating a password (or deleting an operator)
  invalidates every existing session for that operator on the next
  request because `axum-login` re-checks the hash against the
  current operator record. Sessions do not survive a daemon
  restart — operators re-authenticate on reboot.

  CSRF protection uses [`axum_csrf`]'s synchronizer-token pattern:
  the backend issues a random token bound to the `__Host-uw-csrf`
  cookie and an HMAC-derived authenticity token in the
  `X-CSRF-Token` response header. The SPA captures that token from
  responses and echoes it back as `X-CSRF-Token` on every
  state-changing request; the server re-derives the HMAC from the
  cookie and verifies. A per-username login rate limit
  (5 attempts / 60 s) plus a constant-time Argon2id verify against
  a fixed dummy hash on unknown usernames close the enumeration /
  brute-force oracles.

The backend holds no CA private key. Runtime client identity is purely
a SHA-256 fingerprint lookup against the per-instance
`clients/<id>.json` registry — there is no enrollment CA, no CSR signing
path, and no `client_ca_file` to manage.

---

## Instance isolation

Each named instance gets isolated:

- daemon user
- instance group (one per instance — the kernel gate)
- wrapper socket
- runtime directory
- state directory
- systemd service unit

The goal is to make separate instances structurally separate by default rather than relying only on policy checks inside one giant shared namespace.

---

## Encrypted backend key handling

The 32-byte master key for an encrypted store lives **only** in the
backend process's memory. It is generated by `OsRng` at store creation
time, returned to the admin once in the create-store response, and
moved into the `EncryptedBackend`'s `MasterKey` field — a
`Zeroize + ZeroizeOnDrop` struct that scrubs its bytes when the
backend's `Arc` reaches zero references (on lock or shutdown).

Concretely:

- No key file is ever written to disk. There is no `keys_dir`
  configuration knob, no `<keys_dir>/<store>.key`, no rotation-time
  copy on the filesystem. `find /var/lib/underwrap-backend -name
  '*.key'` should return nothing.
- On store creation the backend writes a small **unlock canary**
  (`__underwrap__/canary` containing a fixed plaintext) into the data
  directory under the freshly generated key. The canary is filtered
  from the admin "has secrets" probe and from runtime `list("")`.
- After every backend restart, every encrypted store boots in the
  `Locked` state. Until an operator calls `POST
  /v1/admin/stores/{id}/unlock` and supplies the hex master key, all
  runtime and admin secret operations against the store fail with
  `503 store_locked`. The wrapper-side translates this to a clear
  one-line stderr message: `underwrap: backing store is locked; ask
  an administrator to unlock it`.
- The unlock handler validates the supplied key by decrypting the
  canary. Wrong keys never transition the registry to `Unlocked`; the
  store stays locked and the response carries the structured
  `key_mismatch` code.
- `rotate-key` mints a fresh key, decrypts every resource under the
  old one, re-encrypts under the new one in a staging directory, and
  swaps the data directory atomically. The old `MasterKey` is dropped
  (and therefore zeroized) once the registry's `Arc` is replaced.

Threat-model summary:

- An attacker with read access to the data directory (backup tape,
  leaked image, accidental `scp`) cannot decrypt anything without the
  operator-held hex key. The pre-rewrite design lost this property
  because the key file sat next to the ciphertext.
- An attacker with memory-dump access to the running backend can
  still recover the key — this is out of scope for the
  software-only model. Hardware-backed key custody (TPM, HSM) is not
  part of this design.
- The operator is solely responsible for storing their hex key in a
  password manager or other offline vault. There is no recovery path
  if the key is lost; the data is unrecoverable.

---

## Libsecret mode: the hard part

Libsecret mode is where underwrap's design gets most intricate.

### The problem

A tool such as `gh` may expect a working Secret Service provider reachable through D-Bus.

If it talks to the host's real session bus and real keyring, secret state escapes underwrap's storage model.

So underwrap needs to provide:

- a private bus
- a private Secret Service implementation
- a way for that implementation to reach the backend without authenticating as a normal wrapper again

### The current solution in one sentence

During the authenticated wrapper request, the daemon creates a scoped socketpair and passes one end into the wrapper namespace for `underwrap-secretsd`.

That channel then becomes the long-lived authenticated path between `underwrap-secretsd` and the daemon.

### Why create the channel during the wrapper request?

Because `underwrap-secretsd` itself runs as the ordinary user inside the namespace. It does not have the wrapper's special setgid identity and should not pretend to.

So the right time to authenticate is **before** entering that phase, while the original wrapper request is still authenticated.

### Why this is reasonably strong

The channel is:

- **anonymous** — created by `socketpair()`, not by a discoverable filesystem socket path
- **scoped** — the daemon side binds it to one wrapper's libsecret namespace
- **lifetime-bound** — when the namespace dies, the channel dies

### Daemon-side scoping

The daemon handler for this channel only allows operations inside the wrapper's own libsecret prefix.

That means a compromised `underwrap-secretsd` for one wrapper should not be able to ask the daemon for arbitrary unrelated secrets.

### Descriptor hygiene

A critical rule here is that the final target process must **not** inherit this channel fd.

Only `underwrap-secretsd` should keep it.

If that descriptor leaked to the target, the target would gain a much more direct authenticated path back to the daemon than intended.

---

## Libsecret mode: private D-Bus and mount isolation

A private Secret Service implementation only helps if the target actually talks to that private service.

### The approach

In libsecret mode, `underwrap-ns` launches:

- a private `dbus-daemon`
- `underwrap-secretsd`
- the target process with `DBUS_SESSION_BUS_ADDRESS` pointing at that private bus

### Why mount isolation matters

The setup also uses a mount namespace so that the private bus socket, proc view, and any namespace-private staged filesystem credentials live in a namespace-private filesystem context.

Important steps include:

1. making mounts private
2. mounting a fresh `/proc` inside the PID namespace for libsecret mode
3. mounting private tmpfs roots for namespace-private runtime state such as the D-Bus socket and staged directory credentials
4. using a `noswap` tmpfs for staged directory credentials so those staged files do not page out to swap when the kernel supports the mount option
5. hiding `/run/user/<uid>/bus` if it exists

These steps are not decorative. They are part of keeping the host's normal session bus — and the host's normal home-directory view — from becoming the path of least resistance.

For directory credentials, the same mount-namespace isolation is what keeps backend-managed files out of the host's ordinary home-directory view. Another same-UID host process should not be able to read those mounted files directly from the host namespace while the wrapper runs.

The implementation also treats managed directory trees strictly: it persists only regular files, reconstructs empty directories as needed, and rejects symlinks created inside managed trees during sync.

Like the rest of underwrap's security story, this still depends on kernel behavior. If a deployment kernel does not support the tmpfs options underwrap relies on for directory staging, wrapper execution aborts rather than silently falling back to a weaker mode.

### A real limitation: abstract D-Bus sockets

Filesystem hiding works for filesystem socket paths. It does not block abstract Unix sockets, which live in a different namespace model.

That means there is still a theoretical defense-in-depth gap if some real host session bus were reachable through an abstract socket and a client deliberately ignored the provided `DBUS_SESSION_BUS_ADDRESS`.

In practice, the normal modern session-bus setup on NixOS is path-based and the intended clients are expected to use the provided address. Still, the limitation is worth naming explicitly.

---

## Approval and policy gating model

Whether a wrapper can serve a request is decided by the backend-owned
**policy document**. Operators edit it through the admin HTTP API or
the admin UI; wrappers never see or control it. The document lives
under a reserved key in the same `StorageBackend` that holds the
wrapper's secrets, so policy and secret material share one trust
model.

The document is a flat list of **wrapper permissions** keyed by
`(client_instance, wrapper, resource)`. Each permission carries one of
four modes and a per-permission `writable: bool` flag:

- **`Locked`** — every request through this permission is denied.
  The permission's free-text `reason` is surfaced verbatim to the
  caller's `Deny.reason` payload.
- **`Unlocked`** — `Real` requests are allowed; `Debug` is denied.
- **`UnlockedUntil { until_epoch }`** — like `Unlocked` until the
  deadline. The deadline is checked lazily on each request; there is no
  backend sweeper.
- **`UnsafeDebugUntil { until_epoch }`** — allows **both** the `Real`
  wrapper and its `Debug` variant until the deadline. This is the only
  mode under which the debug tool runs.

`Real` vs `Debug` is the invocation variant the daemon relays
(`uw-<instance> <wrapper>` vs `uw-<instance> debug <wrapper>`). The
daemon — not the caller — picks the target binary, and the backend is
the sole authority for the (mode × variant) matrix; deny reasons are
stable kebab tokens (`locked`, `expired`, `not-unsafe-debug`):

| mode | `Real` | `Debug` |
|------|--------|---------|
| `Locked` | deny (`locked`/reason) | deny (`locked`/reason) |
| `Unlocked` | allow | deny (`not-unsafe-debug`) |
| `UnlockedUntil` active | allow | deny (`not-unsafe-debug`) |
| `UnlockedUntil` expired | deny (`expired`) | deny (`expired`) |
| `UnsafeDebugUntil` active | allow | allow |
| `UnsafeDebugUntil` expired | deny (`expired`) | deny (`expired`) |

Read access through an active permission is unconditional; write access
is the per-permission `writable` flag, re-checked server-side on every
finalize. There is no global write-lock (the old per-store
`writes_enabled` toggle was removed).

**Debug variant.** `UnsafeDebugUntil` unlocks `underwrap-debug-tool`,
which runs against the same projected resource and **prints the
projected secret material to the operator's terminal**. It exists for
incident debugging, not routine use; pick the shortest TTL that fits the
task. The admin UI requires an explicit confirmation before arming this
mode. Switching the permission back to any other mode denies the debug
variant again on the next invocation.

If no permission exists for `(client_instance, wrapper, resource)`
the request is rejected with the structured reason `no-permission`.
The daemon does not distinguish "no permission" from "denied
permission" at the wrapper edge — both surface as
`WrapperResponse::Error { code: Unauthorized }` — so operators can't
be probed for permission existence through the wrapper channel.

Per-invocation approval is intentionally **not** a feature of this
product. Layer human-in-the-loop confirmation at the OS level
(sudo, polkit) in front of the wrapper if you need it.

### Server-side policy authority

Every runtime invocation is re-evaluated on the backend on **both**
the open and the finalize handler. Specifically:

- `POST /v1/wrapper-invocations` resolves the resource by name,
  looks up the pair, looks up the permission keyed by the
  authenticated principal, checks the mode, and replies
  `Allow { resource_id, blob_b64, writable }` or
  `Deny { reason }`.
- `POST /v1/wrapper-invocations/finalize` repeats the same lookup
  and additionally re-checks `writable` against the **current**
  permission for any writeback. A permission flipped from writable
  to read-only between open and finalize is honoured — the
  writeback is rejected with `Deny { reason: "not-writable" }`.
- The new blob is base64-decoded, JCS-canonicalized, checked for
  schema version, and verified against the stored env section.
  Anything that fails surfaces as
  `InvalidBlob { reason: "decode-failed" | "noncanonical-blob" |
  "schema-unknown" | "env-section-immutable" }`.

The daemon carries no policy state and never makes a security
decision — it forwards calls and relays outcomes.

---

## Operational assumptions you should keep in mind

The current design assumes, among other things:

- NixOS-style wrapper installation and service management
- Linux kernel behavior matching the documented mechanisms relied on here
- Yama protections present at the expected setting for the strongest host-side same-UID protection story
- trusted root-owned wrapper configuration on the target host
- sane filesystem permissions on the backend socket in remote mode

If those assumptions change, some of the security conclusions change with them.

---

## Rejected approaches

These ideas were considered and rejected for the current design.

| Approach | Why rejected |
|---|---|
| Treat the client-supplied wrapper name as the *authorization* signal | Spoofable; the name is accepted only as a validated selector behind the kernel egid gate, never as proof of entitlement |
| Trust `argv[0]` for wrapper identity | User-controlled and vulnerable to symlink confusion |
| Identify wrappers by `/proc/<pid>/exe` | All wrapper entry points are fundamentally the same binary shape |
| Put secrets in argv or env between wrapper stages | Too visible in process state |
| Use temp files for handoff | Unnecessary persistence and filesystem exposure |
| Combine setgid and namespace capability in one wrapper stage | Poor fit for the NixOS wrapper model used here |
| Rely on `PR_SET_DUMPABLE=0` alone for the final target | Exec resets the relevant state for ordinary targets |
| Fall back to weaker execution when isolation fails | Silent security degradation |
| Per-wrapper dedicated UIDs for target execution | Breaks ordinary access to the invoking user's working context |
| User-namespace-based redesign | In tension with the current setgid-based identity model |

---

## Final perspective

underwrap is trying to be honest about a difficult reality:

- local CLI execution is messy
- process state is observable in many places
- "just use an env var" is often more ambient than people realize
- agent-facing workflows make that problem more urgent

The current design goes quite far to reduce those problems on NixOS. That is the point of the project.

But the right security attitude is still:

- appreciate the mechanisms
- understand the assumptions
- design narrow wrappers carefully
- invite review instead of assuming the problem is solved forever

One concrete reminder before you ship a wrapper: in env mode the secret
lives in the target's environ for its lifetime, and the default
passthrough list hands the target caller-controlled `TMPDIR`,
`SSH_AUTH_SOCK`, and `DISPLAY`/`WAYLAND_DISPLAY` (see
[Default passthrough trade-offs](#default-passthrough-trade-offs)). Trim
that list to what your target actually needs, and prefer libsecret or
files mode when the target supports it.

## Related docs

- [Architecture](architecture.md)
- [Agent-facing wrappers](agents.md)
- [Remote backend](remote-backend.md)
- [GitHub via libsecret](gh-libsecret.md)
